// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026-present, Dmitry Prudnikov

//! A flush that collects below a watermark must record that it did.
//!
//! `AbstractTree::flush` runs the sealed memtables through the same
//! `CompactionStream` a compaction uses, with the same GC threshold, so it
//! collects the same versions. The version it installs, however, is recorded
//! with `RetentionEffect::Keep`, which leaves the persisted retention floor
//! where it was.
//!
//! While the process lives that is invisible: a read below the install is
//! routed to the retained `SuperVersion` and its sealed memtables. A reopen
//! removes that routing, and the floor is the only boundary left. Admitting a
//! read the flush already collected the answer for makes it return a silent
//! "absent" instead of `SnapshotBelowRetention`, which a consumer cannot tell
//! from a genuine delete.

use lsm_tree::{AbstractTree, Config, SeqNo, SequenceNumberCounter};

#[test]
fn a_flush_that_collects_below_the_watermark_refuses_reads_below_it_after_a_reopen()
-> lsm_tree::Result<()> {
    let dir = tempfile::tempdir()?;

    // The counter sits ABOVE the data seqnos, as any real deployment has it:
    // the install seqno comes from it, and the routing that hides this defect
    // while the process lives depends on the install being above the data.
    let open = || -> lsm_tree::Result<lsm_tree::AnyTree> {
        Config::new(
            dir.path(),
            SequenceNumberCounter::new(100),
            SequenceNumberCounter::new(100),
        )
        .open()
    };

    let tree = open()?;

    // Three versions of one key. Two sit below the watermark used below, so the
    // fold collects the older of those two and keeps the newer; the third is
    // above the watermark and is kept outright.
    tree.insert("k", "oldest", 2);
    tree.insert("k", "middle", 5);
    tree.insert("k", "newest", 10);

    // Establish the reads before the flush, so the test cannot pass by the key
    // never having been readable at these snapshots.
    assert_eq!(
        tree.get("k", 3)?.as_deref(),
        Some(&b"oldest"[..]),
        "precondition: snapshot 3 resolves to the version at seqno 2",
    );
    assert_eq!(
        tree.get("k", 8)?.as_deref(),
        Some(&b"middle"[..]),
        "precondition: snapshot 8 resolves to the version at seqno 5",
    );

    // Flush with a GC watermark of 8. The fold drops the version at seqno 2,
    // which is exactly what snapshot 3 resolved to.
    tree.flush_active_memtable(8)?;

    // The install has to record that, the same way a compaction at this
    // watermark does: a watermark of 8 makes every snapshot below 8 unservable,
    // so the floor is 7.
    assert_eq!(
        tree.retention_floor(),
        7,
        "a flush that collected below watermark 8 must record a floor of 7",
    );

    // While the process lives the read is still ANSWERED, not refused: the
    // history retains the pre-flush SuperVersion and routes the read to its
    // sealed memtables. The persisted floor is a boundary for what survives a
    // reopen, not a live gate, and `oldest_retained_seqno` is the live one.
    assert_eq!(
        tree.get("k", 3)?.as_deref(),
        Some(&b"oldest"[..]),
        "a live tree still answers from the retained version",
    );

    drop(tree);
    let reopened = open()?;

    assert_eq!(
        reopened.retention_floor(),
        7,
        "the floor a collecting flush recorded must survive the reopen",
    );

    // The read the flush collected the answer for must be REFUSED. Answering it
    // with an absent key is indistinguishable from a genuine delete.
    assert!(
        matches!(
            reopened.get("k", SeqNo::from(3_u64)),
            Err(lsm_tree::Error::SnapshotBelowRetention { .. })
        ),
        "a read below the floor a collecting flush recorded must be refused, \
         not answered with a missing key",
    );

    // And the boundary: the floor itself is refused, while the smallest
    // admitted snapshot still resolves to the newest version below the
    // watermark, which the fold kept on purpose.
    assert!(
        matches!(
            reopened.get("k", SeqNo::from(7_u64)),
            Err(lsm_tree::Error::SnapshotBelowRetention { .. })
        ),
        "a read AT the floor must be refused",
    );
    assert_eq!(
        reopened.get("k", 8)?.as_deref(),
        Some(&b"middle"[..]),
        "the smallest admitted snapshot must still resolve to the newest \
         version below the watermark",
    );

    Ok(())
}

#[test]
fn a_flush_at_watermark_zero_leaves_the_floor_alone() -> lsm_tree::Result<()> {
    let dir = tempfile::tempdir()?;

    let open = || -> lsm_tree::Result<lsm_tree::AnyTree> {
        Config::new(
            dir.path(),
            SequenceNumberCounter::new(100),
            SequenceNumberCounter::new(100),
        )
        .open()
    };

    let tree = open()?;

    tree.insert("k", "oldest", 2);
    tree.insert("k", "middle", 5);
    tree.insert("k", "newest", 10);

    // Watermark 0 collects nothing, so it must move nothing: the whole history
    // stays servable, which is what every in-crate flush relies on.
    tree.flush_active_memtable(0)?;

    assert_eq!(tree.retention_floor(), 0);

    drop(tree);
    let reopened = open()?;

    assert_eq!(reopened.retention_floor(), 0);
    assert_eq!(reopened.get("k", 3)?.as_deref(), Some(&b"oldest"[..]));
    assert_eq!(reopened.get("k", 8)?.as_deref(), Some(&b"middle"[..]));
    assert_eq!(
        reopened.get("k", SeqNo::MAX)?.as_deref(),
        Some(&b"newest"[..])
    );

    Ok(())
}
