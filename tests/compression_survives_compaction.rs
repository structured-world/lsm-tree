// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026-present, Dmitry Prudnikov

//! Does the data survive a compaction, for every block codec we ship?
//!
//! The existing codec tests write each key once and read it back. That misses
//! the case a codec change is most likely to break, because compaction is where
//! blocks are decoded and re-encoded rather than merely written: a block decoded
//! wrongly during a merge does not usually lose a key, it resolves the WRONG
//! VERSION of one, and a test that only asks "is the key still there" passes
//! over it.
//!
//! So each scenario here builds several SSTs that disagree about the same keys,
//! then checks the exact expected value of every key three times: before the
//! compaction, after it, and after closing and reopening the tree. The reopen
//! matters separately because recovery re-reads the block headers, including the
//! dictionary id, from files the compaction rewrote.

#![cfg(feature = "zstd")]

use lsm_tree::config::CompressionPolicy;
use lsm_tree::{
    AbstractTree, CompressionType, Config, KvSeparationOptions, SequenceNumberCounter,
    ZstdDictionary,
};
use std::collections::BTreeMap;
use std::sync::Arc;

const N: u32 = 300;

fn key(i: u32) -> Vec<u8> {
    format!("key-{i:05}").into_bytes()
}

/// Values are padded and repetitive on purpose: a codec needs something to
/// compress, and the dictionary below is trained on this same shape.
fn val(i: u32, generation: u8) -> Vec<u8> {
    format!("value-{i:05}-gen{generation}-padding-to-make-it-longer").into_bytes()
}

fn make_dictionary() -> ZstdDictionary {
    let mut samples = Vec::new();
    for i in 0..N {
        samples.extend_from_slice(&key(i));
        samples.extend_from_slice(&val(i, 0));
    }
    ZstdDictionary::new(&samples)
}

/// The write pattern every scenario replays. Three flushes that deliberately
/// disagree, so the merge has real version resolution to do:
///
/// 1. every key at generation 0
/// 2. every third key overwritten at generation 1, every fifth key removed
/// 3. every seventh key overwritten at generation 2
///
/// Returns what a correct engine must answer for each key afterwards.
fn write_three_disagreeing_ssts(
    tree: &lsm_tree::AnyTree,
) -> lsm_tree::Result<BTreeMap<u32, Option<Vec<u8>>>> {
    let mut expected: BTreeMap<u32, Option<Vec<u8>>> = BTreeMap::new();

    let mut seqno = 0_u64;
    for i in 0..N {
        tree.insert(key(i), val(i, 0), seqno);
        expected.insert(i, Some(val(i, 0)));
        seqno += 1;
    }
    tree.flush_active_memtable(0)?;

    for i in (0..N).step_by(3) {
        tree.insert(key(i), val(i, 1), seqno);
        expected.insert(i, Some(val(i, 1)));
        seqno += 1;
    }
    for i in (0..N).step_by(5) {
        tree.remove(key(i), seqno);
        expected.insert(i, None);
        seqno += 1;
    }
    tree.flush_active_memtable(0)?;

    for i in (0..N).step_by(7) {
        tree.insert(key(i), val(i, 2), seqno);
        expected.insert(i, Some(val(i, 2)));
        seqno += 1;
    }
    tree.flush_active_memtable(0)?;

    Ok(expected)
}

fn assert_matches(
    tree: &lsm_tree::AnyTree,
    expected: &BTreeMap<u32, Option<Vec<u8>>>,
    stage: &str,
) -> lsm_tree::Result<()> {
    for (i, want) in expected {
        let got = tree.get(key(*i), lsm_tree::MAX_SEQNO)?;
        match want {
            Some(want) => {
                let got = got.unwrap_or_else(|| panic!("{stage}: key {i} missing"));
                assert_eq!(
                    got.as_ref(),
                    want.as_slice(),
                    "{stage}: wrong value for key {i}"
                );
            }
            None => assert!(got.is_none(), "{stage}: key {i} should have been removed"),
        }
    }
    Ok(())
}

/// Runs the whole write / compact / reopen cycle under one compression policy.
///
/// `dict` is passed separately from the policy because a dictionary lives on the
/// config, not in the codec tag, and reopening has to supply the same one.
fn survives_compaction_and_reopen(
    label: &str,
    policy: CompressionPolicy,
    dict: Option<Arc<ZstdDictionary>>,
) -> lsm_tree::Result<()> {
    let dir = tempfile::tempdir()?;

    let open = |dict: Option<Arc<ZstdDictionary>>| -> lsm_tree::Result<lsm_tree::AnyTree> {
        let config = Config::new(
            dir.path(),
            SequenceNumberCounter::default(),
            SequenceNumberCounter::default(),
        )
        .data_block_compression_policy(policy.clone());
        let config = match dict {
            Some(dict) => config.zstd_dictionary(Some(dict)),
            None => config,
        };
        config.open()
    };

    let tree = open(dict.clone())?;
    let expected = write_three_disagreeing_ssts(&tree)?;
    assert_matches(&tree, &expected, &format!("{label}: before compaction"))?;

    assert!(
        tree.table_count() >= 3,
        "{label}: expected at least 3 tables before compaction, got {}",
        tree.table_count()
    );

    tree.major_compact(u64::MAX, 0)?;

    // Guard the guard: if major_compact ever became a no-op the reads below
    // would be served by the original L0 tables and prove nothing about the
    // re-encoded output.
    assert_eq!(
        Some(0),
        tree.level_table_count(0),
        "{label}: L0 must be empty after major_compact"
    );
    // And the output really landed on a deeper level, which is what makes the
    // per-level policy scenario below a codec TRANSITION rather than a rewrite
    // at the same setting. Without this the whole file could pass vacuously.
    let destination = (1..8).find(|idx| tree.level_table_count(*idx).unwrap_or(0) > 0);
    assert!(
        destination.is_some(),
        "{label}: no level below L0 holds the compacted output"
    );

    assert_matches(&tree, &expected, &format!("{label}: after compaction"))?;

    drop(tree);
    let reopened = open(dict)?;
    assert_matches(&reopened, &expected, &format!("{label}: after reopen"))?;

    Ok(())
}

#[test]
fn uncompressed_blocks_survive_compaction_and_reopen() -> lsm_tree::Result<()> {
    survives_compaction_and_reopen("none", CompressionPolicy::all(CompressionType::None), None)
}

#[test]
fn zstd_fast_blocks_survive_compaction_and_reopen() -> lsm_tree::Result<()> {
    survives_compaction_and_reopen(
        "zstd1",
        CompressionPolicy::all(CompressionType::zstd(1)?),
        None,
    )
}

#[test]
fn zstd_max_blocks_survive_compaction_and_reopen() -> lsm_tree::Result<()> {
    survives_compaction_and_reopen(
        "zstd22",
        CompressionPolicy::all(CompressionType::zstd(22)?),
        None,
    )
}

#[test]
fn zstd_dict_blocks_survive_compaction_and_reopen() -> lsm_tree::Result<()> {
    let dict = make_dictionary();
    let policy = CompressionPolicy::all(CompressionType::zstd_dict(3, dict.id())?);
    survives_compaction_and_reopen("zstd_dict", policy, Some(Arc::new(dict)))
}

#[test]
fn blocks_re_encoded_into_a_different_codec_survive_compaction() -> lsm_tree::Result<()> {
    // The level-transition case, which the per-level policy test never reaches
    // because it stops at the flush. Here L0 is dictionary-compressed and the
    // levels below are not, so the compaction must DECODE with the dictionary
    // and RE-ENCODE without it. Getting that pairing wrong is silent: the bytes
    // still decompress, into the wrong content.
    let dict = make_dictionary();
    let policy = CompressionPolicy::new([
        CompressionType::zstd_dict(3, dict.id())?,
        CompressionType::None,
        CompressionType::zstd(1)?,
    ]);
    survives_compaction_and_reopen("dict_L0_then_plain", policy, Some(Arc::new(dict)))
}

#[test]
fn kv_separated_values_survive_compaction_under_compression() -> lsm_tree::Result<()> {
    // KV separation puts the value in a blob file and leaves an indirection in
    // the SST, so the compaction rewrites the two independently. A codec change
    // that broke the blob side would leave the keys and their indirections
    // intact and hand back wrong values.
    let dir = tempfile::tempdir()?;
    let policy = CompressionPolicy::all(CompressionType::zstd(1)?);

    let open = || -> lsm_tree::Result<lsm_tree::AnyTree> {
        Config::new(
            dir.path(),
            SequenceNumberCounter::default(),
            SequenceNumberCounter::default(),
        )
        .data_block_compression_policy(policy.clone())
        // separation_threshold(1) pushes every value out of line, so the blob
        // side is exercised for all N keys rather than a handful of large ones.
        .with_kv_separation(Some(KvSeparationOptions::default().separation_threshold(1)))
        .open()
    };

    let tree = open()?;
    let expected = write_three_disagreeing_ssts(&tree)?;
    assert_matches(&tree, &expected, "blob: before compaction")?;

    // Same guards the non-blob cases carry. `major_compact` has successful
    // no-op paths, and without these the three L0 tables would happily serve
    // every read below, proving nothing about the rewritten output.
    assert!(
        tree.table_count() >= 3,
        "blob: expected at least 3 tables before compaction, got {}",
        tree.table_count()
    );

    tree.major_compact(u64::MAX, 0)?;

    assert_eq!(
        Some(0),
        tree.level_table_count(0),
        "blob: L0 must be empty after major_compact"
    );
    assert!(
        (1..8).any(|idx| tree.level_table_count(idx).unwrap_or(0) > 0),
        "blob: no level below L0 holds the compacted output"
    );

    assert_matches(&tree, &expected, "blob: after compaction")?;

    drop(tree);
    let reopened = open()?;
    assert_matches(&reopened, &expected, "blob: after reopen")?;

    Ok(())
}
