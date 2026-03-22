/// Regression: point read returns stale value after insert-flush-compact cycles.
mod common;

use lsm_tree::{AbstractTree, Config, Guard, SequenceNumberCounter};

/// Bisect: what's the minimal sequence that triggers the bug?
/// The key insight: compact zeros seqno, then a flushed table with seqno=0
/// competes with a memtable entry at a higher seqno.
#[test]
fn bisect_compact_flush_stale() -> lsm_tree::Result<()> {
    let tmpdir = lsm_tree::get_tmp_folder();
    let tree = Config::new(
        &tmpdir,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    let key = vec![0u8];
    let val0 = vec![0u8; 8];
    let val1 = vec![1u8; 8];

    // Phase 1: create a compacted table with zeroed seqno
    tree.insert(&key, &val0, 1);
    tree.flush_active_memtable(0)?;
    tree.major_compact(common::COMPACTION_TARGET, 3)?; // GC seqno < 3 → zeros seqno=1

    // Phase 2: create multiple flushed tables with the same key
    tree.insert(&key, &val0, 3);
    tree.flush_active_memtable(0)?;
    tree.insert(&key, &val0, 4);
    tree.flush_active_memtable(0)?;
    tree.insert(&key, &val0, 5);
    tree.flush_active_memtable(0)?;

    // Phase 3: compact again to merge everything
    tree.major_compact(common::COMPACTION_TARGET, 6)?;

    // Phase 4: new flushed entry + memtable entries
    tree.insert(&key, &val0, 6);
    tree.flush_active_memtable(0)?;

    tree.insert(&key, &val0, 7);
    tree.insert(&key, &val1, 8); // latest value

    let actual = tree.get(&key, 9)?;
    assert_eq!(
        actual.as_ref().map(|v| v.to_vec()),
        Some(val1),
        "Should see seqno=8 (val=1), not stale entry",
    );

    Ok(())
}
