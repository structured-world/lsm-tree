use lsm_tree::{AbstractTree, SeqNo, SequenceNumberCounter, get_tmp_folder};
use std::sync::Arc;
use test_log::test;

#[test]
fn tree_flush_eviction_1() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();

    let path = folder.path();

    let tree = lsm_tree::Config::new(
        path,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    tree.insert("a", "a", 0);
    tree.remove_weak("a", 1);
    assert_eq!(0, tree.len(SeqNo::MAX, None)?);

    // NOTE: Should not evict weak tombstone
    tree.flush_active_memtable(0)?;
    assert_eq!(1, tree.table_count());
    assert_eq!(0, tree.len(SeqNo::MAX, None)?);

    Ok(())
}

#[test]
fn tree_flush_eviction_2() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();

    let path = folder.path();

    let tree = lsm_tree::Config::new(
        path,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    tree.insert("a", "a", 0);
    tree.remove_weak("a", 1);
    assert_eq!(0, tree.len(SeqNo::MAX, None)?);

    // NOTE: Should evict old value, thus weak tombstone too
    // The watermark has to clear the weak tombstone's OWN seqno (1), not just
    // the value's: at watermark 1 a read at snapshot 1 still resolves to the
    // value, since the delete above it is not visible there yet.
    tree.flush_active_memtable(2)?;
    assert_eq!(0, tree.table_count());
    assert_eq!(0, tree.len(SeqNo::MAX, None)?);

    Ok(())
}

#[test]
fn tree_flush_eviction_3() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();

    let path = folder.path();

    let tree = lsm_tree::Config::new(
        path,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    tree.insert("a", "a", 0);
    tree.remove("a", 1);
    assert_eq!(0, tree.len(SeqNo::MAX, None)?);

    // NOTE: Should evict old value, but tombstone should stay until last level
    tree.flush_active_memtable(1)?;
    assert_eq!(1, tree.table_count());
    assert_eq!(0, tree.len(SeqNo::MAX, None)?);

    // NOTE: Should evict tombstone because last level
    // Bottom-level eviction is bounded by the watermark: the tombstone at seqno
    // 1 leaves only once the watermark is above it, which is also what the
    // threshold-0 contract (collect nothing) requires.
    tree.compact(Arc::new(lsm_tree::compaction::PullDown(0, 6)), 2)?;
    assert_eq!(0, tree.table_count());
    assert_eq!(0, tree.len(SeqNo::MAX, None)?);

    Ok(())
}

#[test]
fn tree_flush_eviction_4() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();

    let path = folder.path();

    let tree = lsm_tree::Config::new(
        path,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    tree.insert("a", "a", 0);
    tree.remove("a", 1);
    tree.insert("a", "a", 2);
    assert_eq!(1, tree.len(SeqNo::MAX, None)?);

    // NOTE: Tombstone should stay because of seqno threshold
    tree.flush_active_memtable(1)?;
    assert_eq!(1, tree.table_count());
    assert_eq!(1, tree.len(SeqNo::MAX, None)?);
    assert_eq!(
        1,
        tree.current_version()
            .level(0)
            .expect("should exist")
            .first()
            .expect("should have at least 1 run")
            .first()
            .expect("should have one table")
            .metadata
            .tombstone_count
    );

    // NOTE: Should evict tombstone because last level
    // Same bound as above: the tombstone sits at seqno 1, so the watermark has
    // to be past it before the bottom level may take it and the value it
    // shadows.
    tree.compact(Arc::new(lsm_tree::compaction::PullDown(0, 6)), 2)?;
    assert_eq!(1, tree.table_count());
    assert_eq!(1, tree.len(SeqNo::MAX, None)?);
    assert_eq!(
        0,
        tree.current_version()
            .level(6)
            .expect("should exist")
            .first()
            .expect("should have at least 1 run")
            .first()
            .expect("should have one table")
            .metadata
            .tombstone_count
    );

    Ok(())
}
