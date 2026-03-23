/// Regression test for <https://github.com/structured-world/lsm-tree/issues/115>.
///
/// The proptest oracle detected that a point read can return a stale value
/// after flushing the same key with different values across two flush cycles.
///
/// Root cause: arena cursor corruption when an allocation fills a block
/// exactly to `BLOCK_SIZE` (fixed in #130 / 5e1eb1b4). The bitwise OR in
/// `(block_idx << BLOCK_SHIFT) | new_end` wraps the cursor back to offset 0
/// of the current block, causing subsequent allocations to overwrite existing
/// skiplist node data. This corrupts memtable entries visible to the flush
/// writer, which then persists stale values to the L0 SST.
///
/// The bug only manifested on i686 targets (4 MiB arena blocks, ~10 block
/// boundaries per million entries) during CI cross-compilation tests. On
/// x86_64 (64 MiB blocks) a single memtable rarely fills even one block,
/// making the exact-boundary condition extremely unlikely.
///
/// This test exercises the minimal operation sequence from the proptest
/// shrunk case as a guard against future regressions:
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
fn stale_point_read_after_two_flushes() -> lsm_tree::Result<()> {
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
