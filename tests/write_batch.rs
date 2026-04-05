use lsm_tree::{AbstractTree, Config, SequenceNumberCounter, WriteBatch, get_tmp_folder};
use test_log::test;

#[test]
fn write_batch_insert_and_read() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    let mut batch = WriteBatch::new();
    batch.insert("a", "val_a");
    batch.insert("b", "val_b");
    batch.insert("c", "val_c");

    let (bytes_added, _memtable_size) = tree.apply_batch(batch, 0);
    assert!(bytes_added > 0);

    assert_eq!(tree.get("a", 1)?.as_deref(), Some(b"val_a".as_slice()));
    assert_eq!(tree.get("b", 1)?.as_deref(), Some(b"val_b".as_slice()));
    assert_eq!(tree.get("c", 1)?.as_deref(), Some(b"val_c".as_slice()));

    Ok(())
}

#[test]
fn write_batch_mixed_operations() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    // Pre-insert a value
    tree.insert("existing", "old_value", 0);

    let mut batch = WriteBatch::new();
    batch.insert("new_key", "new_value");
    batch.remove("existing");
    tree.apply_batch(batch, 1);

    assert_eq!(
        tree.get("new_key", 2)?.as_deref(),
        Some(b"new_value".as_slice())
    );
    assert_eq!(tree.get("existing", 2)?, None); // tombstoned

    Ok(())
}

#[test]
fn write_batch_empty_is_noop() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    let batch = WriteBatch::new();
    let (bytes_added, _) = tree.apply_batch(batch, 0);
    assert_eq!(bytes_added, 0);

    Ok(())
}

#[test]
fn write_batch_shared_seqno_atomic_visibility() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    let mut batch = WriteBatch::new();
    batch.insert("x", "vx");
    batch.insert("y", "vy");
    batch.insert("z", "vz");
    tree.apply_batch(batch, 5);

    // At seqno=5, none should be visible (memtable uses seqno-1 as upper bound)
    assert_eq!(tree.get("x", 5)?, None);
    assert_eq!(tree.get("y", 5)?, None);
    assert_eq!(tree.get("z", 5)?, None);

    // At seqno=6, all should be visible atomically
    assert_eq!(tree.get("x", 6)?.as_deref(), Some(b"vx".as_slice()));
    assert_eq!(tree.get("y", 6)?.as_deref(), Some(b"vy".as_slice()));
    assert_eq!(tree.get("z", 6)?.as_deref(), Some(b"vz".as_slice()));

    Ok(())
}

#[test]
fn write_batch_with_capacity() {
    let batch = WriteBatch::with_capacity(100);
    assert!(batch.is_empty());
    assert_eq!(batch.len(), 0);
}

#[test]
fn write_batch_clear() {
    let mut batch = WriteBatch::new();
    batch.insert("a", "b");
    batch.remove("c");
    assert_eq!(batch.len(), 2);

    batch.clear();
    assert!(batch.is_empty());
}

#[test]
fn write_batch_survives_flush() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let tree = Config::new(
        &folder,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;

    let mut batch = WriteBatch::new();
    for i in 0..50u32 {
        batch.insert(format!("key_{i:04}"), format!("val_{i}"));
    }
    tree.apply_batch(batch, 0);
    tree.flush_active_memtable(0)?;

    for i in 0..50u32 {
        let expected = format!("val_{i}");
        assert_eq!(
            tree.get(format!("key_{i:04}"), 1)?.as_deref(),
            Some(expected.as_bytes()),
            "mismatch at key {i} after flush",
        );
    }

    Ok(())
}
