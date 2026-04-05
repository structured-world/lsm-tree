use lsm_tree::{
    AbstractTree, Config, KvSeparationOptions, PinnableSlice, SeqNo, SequenceNumberCounter,
    get_tmp_folder,
};
use test_log::test;

#[test]
fn get_pinned_memtable_returns_owned() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    tree.insert("a", "value_a", 0);

    let result = tree.get_pinned("a", 1)?;
    assert!(result.is_some());

    let ps = result.expect("should exist");
    // Memtable values are always Owned
    assert!(!ps.is_pinned());
    assert_eq!(ps.value(), b"value_a");
    assert_eq!(ps.len(), 7);
    assert!(!ps.is_empty());

    Ok(())
}

#[test]
fn get_pinned_disk_returns_pinned() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    tree.insert("a", "disk_value", 0);
    tree.flush_active_memtable(0)?;

    let result = tree.get_pinned("a", SeqNo::MAX)?;
    assert!(result.is_some());

    let ps = result.expect("should exist");
    // Disk values should be Pinned (block cache)
    assert!(ps.is_pinned());
    assert_eq!(ps.value(), b"disk_value");

    Ok(())
}

#[test]
fn get_pinned_missing_key_returns_none() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    let result = tree.get_pinned("nonexistent", 1)?;
    assert!(result.is_none());

    Ok(())
}

#[test]
fn get_pinned_tombstoned_key_returns_none() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    tree.insert("a", "value", 0);
    tree.remove("a", 1);

    let result = tree.get_pinned("a", 2)?;
    assert!(result.is_none());

    Ok(())
}

#[test]
fn get_pinned_into_value_conversion() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    tree.insert("a", "my_value", 0);

    let ps = tree.get_pinned("a", 1)?.expect("should exist");
    let user_value = ps.into_value();
    assert_eq!(&*user_value, b"my_value");

    Ok(())
}

#[test]
fn get_pinned_matches_get() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    for i in 0..20u32 {
        tree.insert(format!("key_{i:04}"), format!("val_{i}"), u64::from(i));
    }
    tree.flush_active_memtable(0)?;

    // Insert some in memtable too
    for i in 20..30u32 {
        tree.insert(format!("key_{i:04}"), format!("val_{i}"), u64::from(i));
    }

    // Verify get_pinned returns same data as get for all keys
    for i in 0..35u32 {
        let key = format!("key_{i:04}");
        let regular = tree.get(&key, SeqNo::MAX)?;
        let pinned = tree.get_pinned(&key, SeqNo::MAX)?;

        match (&regular, &pinned) {
            (Some(r), Some(p)) => {
                assert_eq!(r.as_ref(), p.value(), "mismatch at key {key}");
            }
            (None, None) => {}
            _ => panic!("get and get_pinned disagree for key {key}"),
        }
    }

    Ok(())
}

#[test]
fn pinnable_slice_partial_eq() {
    let ps = PinnableSlice::owned(b"hello".as_slice().into());
    assert_eq!(ps.value(), b"hello");
    assert!(ps == b"hello".as_slice());
}

#[test]
fn pinnable_slice_debug_format() {
    let ps = PinnableSlice::owned(b"hello".as_slice().into());
    let debug = format!("{ps:?}");
    assert!(debug.contains("Owned"));

    // Clone
    let ps2 = ps.clone();
    assert_eq!(ps2.value(), b"hello");
}

#[test]
fn get_pinned_blob_tree_returns_owned() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .with_kv_separation(Some(KvSeparationOptions {
        separation_threshold: 1,
        ..Default::default()
    }))
    .open()?;

    let big_val = b"x".repeat(500);
    tree.insert("a", big_val.as_slice(), 0);
    tree.flush_active_memtable(0)?;

    // BlobTree uses default get_pinned impl → always Owned
    let ps = tree.get_pinned("a", SeqNo::MAX)?;
    assert!(ps.is_some());
    let ps = ps.expect("should exist");
    assert!(!ps.is_pinned()); // blob-resolved values are Owned
    assert_eq!(ps.value(), big_val.as_slice());

    Ok(())
}

#[test]
fn get_pinned_with_range_tombstone_returns_none() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    tree.insert("a", "val_a", 0);
    tree.insert("b", "val_b", 1);
    tree.insert("c", "val_c", 2);
    tree.remove_range("a", "c", 3); // deletes [a, c)

    let result_a = tree.get_pinned("a", 4)?;
    let result_b = tree.get_pinned("b", 4)?;
    let result_c = tree.get_pinned("c", 4)?;

    assert!(
        result_a.is_none(),
        "a should be suppressed by range tombstone"
    );
    assert!(
        result_b.is_none(),
        "b should be suppressed by range tombstone"
    );
    assert!(
        result_c.is_some(),
        "c is at the exclusive end, not suppressed"
    );
    assert_eq!(result_c.expect("should exist").value(), b"val_c");

    Ok(())
}

#[test]
fn get_pinned_after_compaction_returns_pinned() -> lsm_tree::Result<()> {
    use lsm_tree::compaction::Leveled;
    use std::sync::Arc;

    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    // Write and flush multiple times to create L0 tables
    for batch in 0..3u32 {
        for i in 0..10u32 {
            let key = format!("key_{i:04}");
            let val = format!("val_{batch}_{i}");
            tree.insert(key, val, u64::from(batch * 10 + i));
        }
        tree.flush_active_memtable(0)?;
    }

    // Compact to push data to L1+
    tree.compact(Arc::new(Leveled::default()), SeqNo::MAX)?;

    // Values from compacted L1+ tables should be Pinned
    let ps = tree.get_pinned("key_0005", SeqNo::MAX)?;
    assert!(ps.is_some());
    let ps = ps.expect("should exist");
    assert!(ps.is_pinned(), "compacted L1+ value should be Pinned");

    Ok(())
}

#[test]
fn get_pinned_sealed_memtable_returns_owned() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    tree.insert("a", "val_sealed", 0);

    // Rotate memtable (seals it) without flushing
    tree.rotate_memtable();

    let ps = tree.get_pinned("a", 1)?;
    assert!(ps.is_some());
    let ps = ps.expect("should exist");
    // Sealed memtable values are still Owned (in-memory)
    assert!(!ps.is_pinned());
    assert_eq!(ps.value(), b"val_sealed");

    Ok(())
}
