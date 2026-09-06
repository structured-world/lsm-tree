//! A snapshot read below the oldest retained version must return
//! `Error::SnapshotBelowRetention`, never panic.
//!
//! `maintenance(gc_watermark)` prunes the version history down to the newest
//! version below the watermark. Any read at a snapshot seqno at or below that
//! version's seqno has no retained version to serve it: the engine used to
//! `expect` one and take the whole tree down. Every read surface that resolves
//! a snapshot is exercised here, on both the standard and the KV-separated
//! tree, plus the `clear` path (which drains the history the same way).

mod common;

use lsm_tree::{
    AbstractTree, AnyTree, Config, Error, Guard, SeqNo, SequenceNumberCounter, get_tmp_folder,
};
use test_log::test;

/// A pruned tree: two flushed versions of `"k"`, then a major compaction whose
/// watermark sits above both, so only the second version stays in the history.
///
/// Returns the tree, the seqno of the FIRST write of `"k"` (a snapshot at
/// `first + 1` saw `v1` before the prune) and the seqno counter, plus the
/// temp folder, which must outlive the tree.
fn pruned_tree(kv_separated: bool) -> lsm_tree::Result<PrunedTree> {
    let folder = get_tmp_folder();
    let seqno = SequenceNumberCounter::default();
    let mut config = Config::new(&folder, seqno.clone(), SequenceNumberCounter::default());
    if kv_separated {
        config = config.with_kv_separation(Some(Default::default()));
    }
    let tree = config.open()?;

    let first = seqno.next();
    tree.insert("k", "v1", first);
    tree.flush_active_memtable(0)?;

    let second = seqno.next();
    tree.insert("k", "v2", second);
    tree.flush_active_memtable(0)?;

    // The watermark is above every seqno handed out so far: maintenance keeps
    // the newest version below it (the second flush) and drops the rest.
    tree.major_compact(common::COMPACTION_TARGET, seqno.get())?;

    Ok(PrunedTree {
        tree,
        first,
        seqno,
        _folder: folder,
    })
}

/// A pruned tree with the temp folder that backs it; dropping the folder
/// first would delete the tables under the live tree.
struct PrunedTree {
    tree: AnyTree,
    first: SeqNo,
    seqno: SequenceNumberCounter,
    _folder: tempfile::TempDir,
}

/// Asserts `err` is the retention error for exactly this boundary: the
/// requested seqno AND the oldest retained seqno must both be reported, so a
/// caller can tell how far behind the retention window it is.
fn assert_below_retention(err: Error, requested: SeqNo, oldest: SeqNo) {
    match err {
        Error::SnapshotBelowRetention {
            requested: got_requested,
            oldest_retained,
        } => {
            assert_eq!(got_requested, requested, "requested seqno must round-trip");
            assert_eq!(
                oldest_retained, oldest,
                "oldest_retained must name the history's front"
            );
        }
        other => panic!("expected SnapshotBelowRetention, got {other:?}"),
    }
}

/// The boundary the read APIs enforce is exactly the oldest retained seqno:
/// a read AT it fails, a read one above it succeeds.
#[test]
fn oldest_retained_seqno_after_prune_is_the_read_boundary() -> lsm_tree::Result<()> {
    let PrunedTree {
        tree,
        first,
        seqno,
        _folder,
    } = pruned_tree(false)?;
    let oldest = tree.oldest_retained_seqno();

    // Pruning happened: the first write's version is gone, the boundary is
    // above it and below the counter.
    assert!(
        oldest > first,
        "history must have been pruned past the first write"
    );
    assert!(oldest < seqno.get(), "boundary must stay below the counter");

    assert_below_retention(tree.get("k", oldest).unwrap_err(), oldest, oldest);
    assert_eq!(
        tree.get("k", oldest + 1)?.as_deref(),
        Some(b"v2".as_slice()),
        "one above the boundary is served from the retained version"
    );
    assert_eq!(
        tree.get("k", SeqNo::MAX)?.as_deref(),
        Some(b"v2".as_slice())
    );
    Ok(())
}

/// A snapshot at seqno 0 sees nothing from any version, so it is served
/// (empty) rather than rejected, even after pruning.
#[test]
fn read_at_seqno_zero_after_prune_is_empty_not_error() -> lsm_tree::Result<()> {
    let PrunedTree { tree, _folder, .. } = pruned_tree(false)?;
    assert!(tree.get("k", 0)?.is_none());
    assert_eq!(tree.len(0, None)?, 0);
    assert!(tree.is_empty(0, None)?);
    Ok(())
}

/// Every point-style read surface on the standard tree reports the error.
#[test]
fn point_reads_below_retention_return_error() -> lsm_tree::Result<()> {
    let PrunedTree {
        tree,
        first,
        _folder,
        ..
    } = pruned_tree(false)?;
    let oldest = tree.oldest_retained_seqno();
    let below = first + 1;

    assert_below_retention(tree.get("k", below).unwrap_err(), below, oldest);
    assert_below_retention(tree.contains_key("k", below).unwrap_err(), below, oldest);
    assert_below_retention(tree.size_of("k", below).unwrap_err(), below, oldest);
    // Both multi_get paths: the small-batch per-key path and the batched path.
    assert_below_retention(tree.multi_get(["k"], below).unwrap_err(), below, oldest);
    assert_below_retention(
        tree.multi_get(["a", "k", "z"], below).unwrap_err(),
        below,
        oldest,
    );
    assert_below_retention(
        tree.approximate_range_stats::<&str, _>(.., below)
            .unwrap_err(),
        below,
        oldest,
    );
    assert_below_retention(
        tree.approximate_range_cardinality::<&str, _>(.., below)
            .unwrap_err(),
        below,
        oldest,
    );
    Ok(())
}

/// Every iterator surface yields the error as its first (and only) item, from
/// both ends, instead of panicking at construction.
#[test]
fn iterators_below_retention_yield_error_item() -> lsm_tree::Result<()> {
    let PrunedTree {
        tree,
        first,
        _folder,
        ..
    } = pruned_tree(false)?;
    let oldest = tree.oldest_retained_seqno();
    let below = first + 1;

    let mut iter = tree.iter(below, None);
    assert_below_retention(
        iter.next().expect("error item").key().unwrap_err(),
        below,
        oldest,
    );
    assert!(iter.next().is_none(), "the error is the only item");

    let mut range = tree.range::<&str, _>(.., below, None);
    assert_below_retention(
        range.next_back().expect("error item").key().unwrap_err(),
        below,
        oldest,
    );
    assert!(range.next_back().is_none());

    let mut prefix = tree.prefix("k", below, None);
    assert_below_retention(
        prefix.next().expect("error item").key().unwrap_err(),
        below,
        oldest,
    );
    assert!(prefix.next().is_none());

    assert_below_retention(tree.len(below, None).unwrap_err(), below, oldest);
    assert_below_retention(tree.is_empty(below, None).unwrap_err(), below, oldest);
    assert_below_retention(
        tree.first_key_value(below, None)
            .expect("error item")
            .key()
            .unwrap_err(),
        below,
        oldest,
    );
    assert_below_retention(
        tree.last_key_value(below, None)
            .expect("error item")
            .key()
            .unwrap_err(),
        below,
        oldest,
    );
    Ok(())
}

/// The seekable iterator and the batch scan share the seekable pipeline; both
/// surface the error once, and a seek on the failed iterator is a harmless
/// no-op rather than a panic.
#[test]
fn seekable_and_batch_scan_below_retention_yield_error_item() -> lsm_tree::Result<()> {
    let PrunedTree {
        tree,
        first,
        _folder,
        ..
    } = pruned_tree(false)?;
    let oldest = tree.oldest_retained_seqno();
    let below = first + 1;

    let mut seekable = tree.range_seekable::<&str, _>(.., below, None);
    seekable.seek_to(b"a");
    assert_below_retention(
        seekable.peek_key().expect("error item").unwrap_err(),
        below,
        oldest,
    );
    // `peek_key` consumes an error (documented); the iterator is then done.
    assert!(seekable.next().is_none());
    seekable.seek_to_for_prev(b"z");
    assert!(seekable.next_back().is_none());

    let mut batch = tree.batch_range_scan(["a".."m", "m".."z"], below, None);
    assert_below_retention(
        batch.next().expect("error item").key().unwrap_err(),
        below,
        oldest,
    );
    assert!(batch.next().is_none());
    Ok(())
}

/// The columnar scan resolves its snapshot up front and fails there.
#[cfg(feature = "columnar")]
#[test]
fn columnar_scan_below_retention_returns_error() -> lsm_tree::Result<()> {
    let PrunedTree {
        tree,
        first,
        _folder,
        ..
    } = pruned_tree(false)?;
    let oldest = tree.oldest_retained_seqno();
    let below = first + 1;
    let AnyTree::Standard(tree) = tree else {
        panic!("pruned_tree(false) opens a standard tree");
    };
    let err = match tree.columnar_scan(&[], None, below, ..) {
        Ok(_) => panic!("columnar scan below retention must fail"),
        Err(e) => e,
    };
    assert_below_retention(err, below, oldest);
    Ok(())
}

/// The KV-separated tree resolves snapshots through its index tree and has
/// its own read implementations; each of them must report the error too.
#[test]
fn blob_tree_reads_below_retention_return_error() -> lsm_tree::Result<()> {
    let PrunedTree {
        tree,
        first,
        _folder,
        ..
    } = pruned_tree(true)?;
    let oldest = tree.oldest_retained_seqno();
    let below = first + 1;

    assert_below_retention(tree.get("k", below).unwrap_err(), below, oldest);
    assert_below_retention(tree.multi_get(["k"], below).unwrap_err(), below, oldest);
    assert_below_retention(
        tree.multi_get(["a", "k", "z"], below).unwrap_err(),
        below,
        oldest,
    );

    let mut range = tree.range::<&str, _>(.., below, None);
    assert_below_retention(
        range.next().expect("error item").key().unwrap_err(),
        below,
        oldest,
    );
    assert!(range.next().is_none());

    let mut prefix = tree.prefix("k", below, None);
    assert_below_retention(
        prefix.next_back().expect("error item").key().unwrap_err(),
        below,
        oldest,
    );
    assert!(prefix.next_back().is_none());

    let mut seekable = tree.range_seekable::<&str, _>(.., below, None);
    assert_below_retention(
        seekable.next().expect("error item").key().unwrap_err(),
        below,
        oldest,
    );
    assert!(seekable.next().is_none());

    let mut batch = tree.batch_range_scan(["a".."z"], below, None);
    assert_below_retention(
        batch.next().expect("error item").key().unwrap_err(),
        below,
        oldest,
    );
    assert!(batch.next().is_none());

    // Above the boundary the retained version serves the read.
    assert_eq!(
        tree.get("k", oldest + 1)?.as_deref(),
        Some(b"v2".as_slice())
    );
    Ok(())
}

/// `clear` drains the history to the (new, empty) latest version, so a
/// snapshot taken before the clear is below retention afterwards.
#[test]
fn read_at_pre_clear_snapshot_after_clear_returns_error() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let seqno = SequenceNumberCounter::default();
    let tree = Config::new(&folder, seqno.clone(), SequenceNumberCounter::default()).open()?;

    tree.insert("k", "v1", seqno.next());
    let snapshot = seqno.get();
    assert_eq!(tree.get("k", snapshot)?.as_deref(), Some(b"v1".as_slice()));

    tree.clear()?;

    let oldest = tree.oldest_retained_seqno();
    assert!(
        oldest >= snapshot,
        "clear installs a version at or above the snapshot"
    );
    assert_below_retention(tree.get("k", snapshot).unwrap_err(), snapshot, oldest);
    assert!(tree.get("k", SeqNo::MAX)?.is_none());
    Ok(())
}

/// Without pruning nothing changes: the initial version (seqno 0) stays at
/// the front, so every snapshot above 0 is served and the boundary reads 0.
#[test]
fn oldest_retained_seqno_without_prune_is_zero() -> lsm_tree::Result<()> {
    let folder = get_tmp_folder();
    let seqno = SequenceNumberCounter::default();
    let tree = Config::new(&folder, seqno.clone(), SequenceNumberCounter::default()).open()?;

    let first = seqno.next();
    tree.insert("k", "v1", first);
    tree.flush_active_memtable(0)?;
    tree.insert("k", "v2", seqno.next());
    tree.flush_active_memtable(0)?;
    // Watermark 0 certifies nothing as collapsible: no version is pruned.
    tree.major_compact(common::COMPACTION_TARGET, 0)?;

    assert_eq!(tree.oldest_retained_seqno(), 0);
    assert_eq!(tree.get("k", first + 1)?.as_deref(), Some(b"v1".as_slice()));
    Ok(())
}
