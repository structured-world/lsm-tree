mod common;
use lsm_tree::{AbstractTree, Config, SequenceNumberCounter};

#[test]
fn with_post_compact_flush() -> lsm_tree::Result<()> {
    let tmpdir = lsm_tree::get_tmp_folder();
    let tree = Config::new(
        &tmpdir,
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .open()?;
    let k = vec![0u8];
    let v0 = vec![0u8; 8];
    let v1 = vec![1u8; 8];

    tree.major_compact(common::COMPACTION_TARGET, 1)?;
    tree.flush_active_memtable(0)?;
    tree.insert(&k, &v0, 1);
    tree.flush_active_memtable(0)?;
    tree.insert(&k, &v0, 2);
    tree.major_compact(common::COMPACTION_TARGET, 3)?;
    tree.major_compact(common::COMPACTION_TARGET, 3)?;
    tree.major_compact(common::COMPACTION_TARGET, 3)?;
    tree.flush_active_memtable(0)?;
    tree.insert(&k, &v0, 3);
    tree.flush_active_memtable(0)?;
    tree.insert(&k, &v0, 4);
    tree.flush_active_memtable(0)?;
    tree.insert(&k, &v0, 5);
    tree.flush_active_memtable(0)?;
    tree.major_compact(common::COMPACTION_TARGET, 6)?;
    // This insert+flush creates an L0 table after second compact
    tree.insert(&k, &v0, 6);
    tree.flush_active_memtable(0)?;
    // Two memtable inserts
    tree.insert(&k, &v0, 7);
    tree.insert(&k, &v1, 8);

    assert_eq!(tree.get(&k, 9)?.as_ref().map(|v| v.to_vec()), Some(v1));
    Ok(())
}
