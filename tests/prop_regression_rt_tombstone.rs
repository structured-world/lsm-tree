// Regression tests found by prop_btreemap_oracle proptest.
//
// BUG 1 (regression_overwrite_across_ssts): When there are 3+ L0 SSTs and
// an active memtable with data for a DIFFERENT key, point reads for keys
// across SSTs return stale values. The L0 merge resolution doesn't correctly
// pick the newest version when the active memtable is non-empty.
//
// BUG 2 (regression_remove_range_then_insert_then_remove): Range tombstone +
// point tombstone interaction across SSTs — the point tombstone is not visible.
//
// TODO: fix the MVCC resolution bugs and remove #[ignore]

use lsm_tree::{AbstractTree, Config, SequenceNumberCounter};

#[test]
#[ignore = "known bug: point tombstone not visible when RT exists in prior SST"]
fn regression_remove_range_then_insert_then_remove() -> lsm_tree::Result<()> {
    let tmpdir = lsm_tree::get_tmp_folder();
    let tree = Config::new(
        &tmpdir,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    // seqno 1: Insert key=[3]
    tree.insert(vec![3u8], vec![0u8], 1);
    tree.flush_active_memtable(0)?;

    // seqno 2-4: Insert key=[0] three times
    tree.insert(vec![0u8], vec![0u8], 2);
    tree.insert(vec![0u8], vec![0u8], 3);
    tree.insert(vec![0u8], vec![0u8], 4);

    // seqno 5: RemoveRange [0, 3) — covers keys 0, 1, 2
    tree.remove_range(&[0u8], &[3u8], 5);

    // seqno 6: Insert key=[2] AFTER the range tombstone
    tree.insert(vec![2u8], vec![0u8], 6);

    tree.flush_active_memtable(0)?;

    // seqno 7: Remove key=[2] — point tombstone
    tree.remove(vec![2u8], 7);

    tree.flush_active_memtable(0)?;

    // Read at seqno 8: key=[2] should be deleted (tombstone at seqno 7)
    let result = tree.get(&[2u8], 8)?;
    assert_eq!(
        result, None,
        "key=[2] should be None at seqno 8 (tombstone at seqno 7 suppresses insert at seqno 6)"
    );

    // Also verify key=[0] is deleted by range tombstone
    let result = tree.get(&[0u8], 8)?;
    assert_eq!(
        result, None,
        "key=[0] should be None at seqno 8 (range tombstone at seqno 5)"
    );

    // key=[3] should still be visible (range tombstone [0,3) doesn't cover key=[3])
    let result = tree.get(&[3u8], 8)?;
    assert_eq!(
        result,
        Some(vec![0u8].into()),
        "key=[3] should be Some at seqno 8 (not covered by range tombstone)"
    );

    Ok(())
}

/// Regression: overwrite across SSTs — tree returns old value.
/// Found by prop_btreemap_oracle (no range tombstones involved).
#[test]
#[ignore = "known bug: L0 MVCC resolution with active memtable"]
fn regression_overwrite_across_ssts() -> lsm_tree::Result<()> {
    let tmpdir = lsm_tree::get_tmp_folder();
    let tree = Config::new(
        &tmpdir,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    // SST-1: key=2@1
    tree.insert(vec![2u8], vec![0u8], 1);
    tree.flush_active_memtable(0)?;

    // SST-2: key=1@2, key=0@3, key=2@4
    tree.insert(vec![1u8], vec![0u8], 2);
    tree.insert(vec![0u8], vec![0u8], 3);
    tree.insert(vec![2u8], vec![0u8], 4);
    tree.flush_active_memtable(0)?;

    // SST-3: key=0@5 (newer version, val=1)
    tree.insert(vec![0u8], vec![1u8], 5);
    tree.flush_active_memtable(0)?;

    // Memtable: key=1@6, key=1@7
    tree.insert(vec![1u8], vec![0u8], 6);
    tree.insert(vec![1u8], vec![0u8], 7);

    // Read key=0 at seqno 8: should see val=1 (from seqno 5, not val=0 from seqno 3)
    let result = tree.get(&[0u8], 8)?;
    assert_eq!(
        result.as_ref().map(|v| v.to_vec()),
        Some(vec![1u8]),
        "key=0 at seqno 8: expected [1] from SST-3 (seqno 5), got old value from SST-2 (seqno 3)"
    );

    Ok(())
}

/// Ultra-minimal: two SSTs, same key, newer value in second SST.
#[test]
fn regression_two_ssts_same_key() -> lsm_tree::Result<()> {
    let tmpdir = lsm_tree::get_tmp_folder();
    let tree = Config::new(
        &tmpdir,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    // SST-1: key=0 val=old at seqno 1
    tree.insert(vec![0u8], vec![0u8], 1);
    tree.flush_active_memtable(0)?;

    // SST-2: key=0 val=new at seqno 2
    tree.insert(vec![0u8], vec![1u8], 2);
    tree.flush_active_memtable(0)?;

    eprintln!("Tables: {}", tree.table_count());

    let result = tree.get(&[0u8], 3)?;
    assert_eq!(
        result.as_ref().map(|v| v.to_vec()),
        Some(vec![1u8]),
        "Should see val=1 (seqno 2) not val=0 (seqno 1)"
    );
    Ok(())
}

/// Three SSTs: key in SST-2, updated in SST-3.
#[test]
fn regression_three_ssts_overwrite() -> lsm_tree::Result<()> {
    let tmpdir = lsm_tree::get_tmp_folder();
    let tree = Config::new(
        &tmpdir,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    // SST-1: unrelated key
    tree.insert(vec![2u8], vec![0u8], 1);
    tree.flush_active_memtable(0)?;

    // SST-2: key=0 val=old at seqno 3
    tree.insert(vec![1u8], vec![0u8], 2);
    tree.insert(vec![0u8], vec![0u8], 3);
    tree.flush_active_memtable(0)?;

    // SST-3: key=0 val=new at seqno 5
    tree.insert(vec![0u8], vec![1u8], 5);
    tree.flush_active_memtable(0)?;

    eprintln!("Tables: {}", tree.table_count());

    let result = tree.get(&[0u8], 8)?;
    assert_eq!(
        result.as_ref().map(|v| v.to_vec()),
        Some(vec![1u8]),
        "Should see val=1 (seqno 5) not val=0 (seqno 3)"
    );
    Ok(())
}

/// Three SSTs + active memtable data.
#[test]
#[ignore = "known bug: L0 MVCC resolution with active memtable"]
fn regression_three_ssts_plus_memtable() -> lsm_tree::Result<()> {
    let tmpdir = lsm_tree::get_tmp_folder();
    let tree = Config::new(
        &tmpdir,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    // SST-1: key=2@1
    tree.insert(vec![2u8], vec![0u8], 1);
    tree.flush_active_memtable(0)?;

    // SST-2: key=1@2, key=0@3, key=2@4
    tree.insert(vec![1u8], vec![0u8], 2);
    tree.insert(vec![0u8], vec![0u8], 3);
    tree.insert(vec![2u8], vec![0u8], 4);
    tree.flush_active_memtable(0)?;

    // SST-3: key=0@5 val=1
    tree.insert(vec![0u8], vec![1u8], 5);
    tree.flush_active_memtable(0)?;

    // Active memtable: key=1@6, key=1@7
    tree.insert(vec![1u8], vec![0u8], 6);
    tree.insert(vec![1u8], vec![0u8], 7);

    eprintln!(
        "Tables: {}, L0 runs: {}",
        tree.table_count(),
        tree.l0_run_count()
    );

    let result = tree.get(&[0u8], 8)?;
    assert_eq!(
        result.as_ref().map(|v| v.to_vec()),
        Some(vec![1u8]),
        "Should see val=1 (seqno 5, SST-3) not val=0 (seqno 3, SST-2)"
    );
    Ok(())
}

/// Verify that a point tombstone across SSTs works WITHOUT range tombstones.
#[test]
fn baseline_point_tombstone_across_ssts() -> lsm_tree::Result<()> {
    let tmpdir = lsm_tree::get_tmp_folder();
    let tree = Config::new(
        &tmpdir,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    tree.insert(vec![2u8], vec![42u8], 1);
    tree.flush_active_memtable(0)?;

    tree.remove(vec![2u8], 2);
    tree.flush_active_memtable(0)?;

    let result = tree.get(&[2u8], 3)?;
    assert_eq!(result, None, "Point tombstone across SSTs should work");
    Ok(())
}

/// Same scenario but with RT in the same SST as the insert.
#[test]
fn regression_rt_same_sst_then_tombstone_in_next() -> lsm_tree::Result<()> {
    let tmpdir = lsm_tree::get_tmp_folder();
    let tree = Config::new(
        &tmpdir,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    // RT and insert in same memtable, flushed together
    tree.remove_range(&[0u8], &[3u8], 1);
    tree.insert(vec![2u8], vec![42u8], 2);
    tree.flush_active_memtable(0)?;

    // Point tombstone in next SST
    tree.remove(vec![2u8], 3);
    tree.flush_active_memtable(0)?;

    let result = tree.get(&[2u8], 4)?;
    assert_eq!(
        result, None,
        "Point tombstone should suppress insert even when RT exists in prior SST"
    );
    Ok(())
}
