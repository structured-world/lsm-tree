// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026-present, Dmitry Prudnikov

//! A compaction must not drop a version that the recorded retention floor still
//! promises to serve.
//!
//! The fold in `CompactionStream` used to decide by the seqno of the NEXT-OLDER
//! sibling: when that one was below the GC threshold, the rest of the key was
//! drained. So for a key whose versions straddle the threshold, the newest
//! version BELOW it was discarded even though a read above the floor resolves
//! to exactly that version.
//!
//! While the process lives that was invisible: such a read is routed to the
//! retained `SuperVersion` and reads the pre-compaction tables. A reopen removes
//! that routing — the history is seeded at the persisted floor and every read
//! resolves against the latest version — so the answer became a silent
//! "absent" rather than either the value or `SnapshotBelowRetention`.

use lsm_tree::{AbstractTree, Config, SeqNo, SequenceNumberCounter};

#[test]
fn a_read_above_the_floor_still_sees_its_version_after_a_reopen() -> lsm_tree::Result<()> {
    let dir = tempfile::tempdir()?;

    // The counter has to sit ABOVE the data seqnos, as it does in any real
    // deployment: the install seqno of a compaction output is taken from it, and
    // the routing that makes the fold sound depends on that install seqno being
    // above every version in the input. A default-constructed counter would put
    // the install at 0, send the read below to the LATEST version, and make this
    // test prove something other than what it claims.
    let open = || -> lsm_tree::Result<lsm_tree::AnyTree> {
        Config::new(
            dir.path(),
            SequenceNumberCounter::new(100),
            SequenceNumberCounter::new(100),
        )
        .open()
    };

    let tree = open()?;

    // Two versions of one key, straddling the GC threshold used below: an old
    // one well under it, a new one well over.
    tree.insert("k", "old", 2);
    tree.flush_active_memtable(0)?;
    tree.insert("k", "new", 10);
    tree.flush_active_memtable(0)?;

    // A snapshot read at 9 sees versions with seqno < 9, so it must answer
    // "old". Establish that before the compaction, so the test cannot pass by
    // the key never having been readable.
    assert_eq!(
        tree.get("k", 9)?.as_deref(),
        Some(&b"old"[..]),
        "precondition: snapshot 9 resolves to the version at seqno 2",
    );

    // Compact with a GC threshold of 8. The fold keeps BOTH versions here: the
    // one at 10 is above the watermark, and the one at 2 is the newest below
    // it. So this compaction collects nothing and records no floor at all, and
    // a read at 9 stays servable for the strongest possible reason.
    tree.major_compact(u64::MAX, 8)?;

    let floor = tree.retention_floor();
    assert_eq!(
        floor, 0,
        "a compaction whose fold collected nothing must not move the floor",
    );

    // Live process: the retained version still routes this read to the old
    // tables, so it answers correctly here.
    assert_eq!(
        tree.get("k", 9)?.as_deref(),
        Some(&b"old"[..]),
        "a read above the floor must still resolve to seqno 2 while the process lives",
    );

    drop(tree);
    let reopened = open()?;

    // After the reopen the retained versions are gone and the floor is the only
    // boundary. The read is still above it, so the engine has promised to serve
    // it: it must answer with the value, and must never answer "absent".
    assert_eq!(
        reopened.retention_floor(),
        0,
        "the floor a non-collecting compaction recorded must survive the reopen",
    );
    assert_eq!(
        reopened.get("k", 9)?.as_deref(),
        Some(&b"old"[..]),
        "after reopen a read above the retention floor answered with a missing key \
         instead of the version the floor promised",
    );

    // Nothing was collected, so nothing is refused: every snapshot down to the
    // one just above the version at seqno 2 still resolves to it.
    assert_eq!(
        reopened.get("k", SeqNo::from(3_u64))?.as_deref(),
        Some(&b"old"[..])
    );

    Ok(())
}
