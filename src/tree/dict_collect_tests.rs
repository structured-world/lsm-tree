// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026-present, Dmitry Prudnikov

//! Dictionary collection against the deletion pause.
//!
//! Lives in the crate rather than beside the other dictionary integration
//! tests because the pause is not part of the public surface.

use crate::compression::ZstdDictionary;
use crate::config::CompressionPolicy;
use crate::{AbstractTree, CompressionType, Config, SequenceNumberCounter};
use std::sync::Arc;
use test_log::test;

fn training_corpus() -> Vec<u8> {
    let mut samples = Vec::new();
    for i in 0u32..500 {
        samples.extend_from_slice(format!("key-{i:05}").as_bytes());
        samples.extend_from_slice(format!("value-{i:05}-padding-to-make-it-longer").as_bytes());
    }
    samples
}

fn config(path: &std::path::Path) -> Config {
    Config::new(
        path,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
}

/// A dictionary a checkpoint may still need must not be unlinked under it.
///
/// The checkpoint holds its captured version as a LOCAL clone, so the history
/// can move past that version while the checkpoint is still going to link the
/// dictionaries it names. Removing one directly would fail `link_dictionaries`
/// on a missing file and abort an otherwise valid checkpoint, so the removal
/// goes through the same deletion pause the tables and blob files use.
#[test]
fn a_collection_under_a_held_pause_defers_the_dictionary_removal() -> crate::Result<()> {
    let dir = tempfile::tempdir()?;
    let dict = ZstdDictionary::new(&training_corpus());
    let dict_id = dict.id();
    let compression = CompressionType::zstd_dict(3, dict_id)?;

    // Written under the dictionary...
    {
        let tree = config(dir.path())
            .data_block_compression_policy(CompressionPolicy::all(compression))
            .zstd_dictionary(Some(Arc::new(dict)))
            .open()?;
        for i in 0u32..100 {
            tree.insert(
                format!("key-{i:05}").as_bytes(),
                b"value-under-the-dictionary",
                u64::from(i),
            );
        }
        tree.flush_active_memtable(0)?;
    }

    // ...then rewritten without it. The first collection only unregisters it
    // from the latest version; the file stays while a retained version names
    // it, so the reopen below is what makes it genuinely removable.
    {
        let tree = config(dir.path())
            .data_block_compression_policy(CompressionPolicy::all(CompressionType::None))
            .open()?;
        tree.major_compact(u64::MAX, 0)?;
        let crate::AnyTree::Standard(tree) = tree else {
            panic!("a standard tree");
        };
        tree.collect_unreferenced_dictionaries()?;
    }

    let tree = config(dir.path())
        .data_block_compression_policy(CompressionPolicy::all(CompressionType::None))
        .open()?;
    let crate::AnyTree::Standard(tree) = tree else {
        panic!("a standard tree");
    };
    let dict_path = dir
        .path()
        .join(crate::file::DICTS_FOLDER)
        .join(dict_id.to_string());
    assert!(dict_path.exists(), "collectable, but still on disk");

    {
        let _pause = tree.deletion_pause.acquire();
        assert_eq!(
            tree.collect_unreferenced_dictionaries()?,
            0,
            "a collection under a held pause removes nothing itself",
        );
        assert!(
            dict_path.exists(),
            "a file a checkpoint may still link is not unlinked under the pause",
        );
    }

    assert!(
        !dict_path.exists(),
        "and the deferred removal is carried out once the pause closes",
    );
    Ok(())
}

/// A collected dictionary leaves the LIVE registry, not just the disk.
///
/// The registry holds each dictionary's raw bytes and prepared decoder state, so
/// a tree that rotates dictionaries would otherwise carry every generation it
/// ever held for its whole life, and keep reporting ids it no longer owns.
#[test]
fn a_collected_dictionary_is_evicted_from_the_live_registry() -> crate::Result<()> {
    let dir = tempfile::tempdir()?;
    let dict = ZstdDictionary::new(&training_corpus());
    let dict_id = dict.id();
    let compression = CompressionType::zstd_dict(3, dict_id)?;

    {
        let tree = config(dir.path())
            .data_block_compression_policy(CompressionPolicy::all(compression))
            .zstd_dictionary(Some(Arc::new(dict)))
            .open()?;
        for i in 0u32..100 {
            tree.insert(
                format!("key-{i:05}").as_bytes(),
                b"value-under-the-dictionary",
                u64::from(i),
            );
        }
        tree.flush_active_memtable(0)?;
    }

    {
        let tree = config(dir.path())
            .data_block_compression_policy(CompressionPolicy::all(CompressionType::None))
            .open()?;
        tree.major_compact(u64::MAX, 0)?;
        let crate::AnyTree::Standard(tree) = tree else {
            panic!("a standard tree");
        };
        tree.collect_unreferenced_dictionaries()?;
    }

    let tree = config(dir.path())
        .data_block_compression_policy(CompressionPolicy::all(CompressionType::None))
        .open()?;
    let crate::AnyTree::Standard(tree) = tree else {
        panic!("a standard tree");
    };
    assert!(
        tree.zstd_dictionaries().get(dict_id).is_some(),
        "loaded at open, before the collection",
    );

    assert_eq!(tree.collect_unreferenced_dictionaries()?, 1);
    assert!(
        tree.zstd_dictionaries().get(dict_id).is_none(),
        "a collected dictionary is dropped from the live set as well as the disk",
    );
    Ok(())
}
