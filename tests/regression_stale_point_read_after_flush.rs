/// Scenario guard for <https://github.com/structured-world/lsm-tree/issues/115>.
///
/// The proptest oracle detected that a point read returned a stale value after
/// flushing the same key with different values across two flush cycles.
///
/// Root cause: arena cursor corruption when an allocation fills a block exactly
/// to `BLOCK_SIZE` (fixed in #130 / 5e1eb1b4). The low-level boundary condition
/// is covered by the unit test `arena::tests::exact_block_fill_does_not_corrupt`
/// in `src/memtable/arena.rs`, which directly triggers the cursor wrap.
///
/// This test does NOT exercise the arena boundary — the operation sequence is
/// too small to fill even a single arena block on any target. Instead it
/// documents the user-visible symptom (stale point read after two flushes) as
/// an end-to-end scenario guard. If the arena regression test passes but this
/// test fails, it signals a different stale-read bug in the flush/L0 pipeline.
///
/// Operation sequence from the proptest shrunk case:
///
/// ```text
/// ops = [
///     Compact, Compact, Compact, Compact, Compact,
///     Insert { key_idx: 0, value: [0] },
///     Insert { key_idx: 0, value: [0] },
///     Flush,
///     Insert { key_idx: 0, value: [1] },
///     Flush,
/// ]
/// ```
mod common;

use lsm_tree::{AbstractTree, Config, SequenceNumberCounter};

#[test]
fn point_read_after_two_flushes_returns_latest_value() -> lsm_tree::Result<()> {
    let tmpdir = lsm_tree::get_tmp_folder();
    let seqno = SequenceNumberCounter::default();
    let visible_seqno = SequenceNumberCounter::default();
    let tree = Config::new(&tmpdir, seqno.clone(), visible_seqno.clone()).open()?;

    let k = vec![0u8];
    let v0 = vec![0u8];
    let v1 = vec![1u8];

    // 5 no-op compacts (empty tree, Choice::DoNothing)
    for _ in 0..5 {
        let gc = seqno.get();
        tree.major_compact(common::COMPACTION_TARGET, gc)?;
    }

    // Two inserts with same key and value [0]
    let s = seqno.next();
    tree.insert(&k, &v0, s);
    visible_seqno.fetch_max(s + 1);

    let s = seqno.next();
    tree.insert(&k, &v0, s);
    visible_seqno.fetch_max(s + 1);

    // Flush → writes both entries to L0 table
    tree.flush_active_memtable(0)?;

    // Insert with value [1] (newer)
    let s = seqno.next();
    tree.insert(&k, &v1, s);
    visible_seqno.fetch_max(s + 1);

    // Second flush → writes the [1] entry to another L0 table
    tree.flush_active_memtable(0)?;

    // Point read must return the latest value
    let read_seqno = visible_seqno.get();
    let actual = tree.get(&k, read_seqno)?.map(|v| v.to_vec());

    assert_eq!(
        actual,
        Some(v1),
        "Point read at seqno={read_seqno} should return [1] (latest), not stale [0]"
    );

    Ok(())
}
