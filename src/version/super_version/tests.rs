use super::*;
use crate::comparator::default_comparator;
use crate::fs::{Fs, FsOpenOptions, MemFs};
use test_log::test;

fn new_memtable(id: u64) -> Memtable {
    Memtable::new(id, default_comparator())
}

fn test_super_versions(versions: Vec<SuperVersion>) -> SuperVersions {
    #[cfg(feature = "std")]
    #[expect(
        clippy::expect_used,
        reason = "test helper: every caller passes a non-empty version list"
    )]
    let latest = Arc::new(ArcSwap::from_pointee(
        versions
            .last()
            .cloned()
            .expect("test helper requires at least one version"),
    ));
    SuperVersions {
        versions: versions.into(),
        comparator_name: "default".into(),
        sync_mode: SyncMode::Normal,
        snapshot_id: 0,
        log_rotate_bytes: 1024 * 1024,
        log_bytes: None,
        edit_scratch: Vec::new(),
        #[cfg(feature = "std")]
        latest,
    }
}

/// Seed version files (`v1`, `v2`, ...) into `fs` at `dir` for each
/// `SuperVersion` in the list. This makes GC tests exercise the real
/// `Fs::remove_file` path instead of only hitting `NotFound`.
fn seed_version_files(dir: &Path, versions: &SuperVersions, fs: &dyn Fs) -> crate::Result<()> {
    fs.create_dir_all(dir)?;
    for sv in &versions.versions {
        let path = dir.join(format!("v{}", sv.version.id()));
        fs.open(
            &path,
            &FsOpenOptions::new().write(true).create(true).truncate(true),
        )?;
    }
    Ok(())
}

#[test]
fn super_version_gc_above_watermark() -> crate::Result<()> {
    let fs = MemFs::new();
    let dir = Path::new("/gc/above");
    let mut history = test_super_versions(vec![
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(1, crate::TreeType::Standard),
            seqno: 0,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(2, crate::TreeType::Standard),
            seqno: 1,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(3, crate::TreeType::Standard),
            seqno: 2,
        },
    ]);
    seed_version_files(dir, &history, &fs)?;

    // gc_watermark=0 → early return, no GC
    history.maintenance(dir, 0, &fs)?;

    assert_eq!(history.free_list_len(), 2);
    // All version files still present (no GC ran)
    assert!(fs.exists(&dir.join("v1"))?);
    assert!(fs.exists(&dir.join("v2"))?);
    assert!(fs.exists(&dir.join("v3"))?);

    Ok(())
}

#[test]
fn super_version_gc_preserves_current_snapshot_file() -> crate::Result<()> {
    // The CURRENT snapshot file must survive GC even when its in-memory
    // version is evicted from the history — `CURRENT` still points at it and
    // the edit log layers on top. Set snapshot_id to a seeded v{id} that GC
    // will evict, and assert its file stays while a non-snapshot evictee is
    // removed.
    let fs = MemFs::new();
    let dir = Path::new("/gc/snapshot");
    let mut history = test_super_versions(vec![
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(1, crate::TreeType::Standard),
            seqno: 0,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(2, crate::TreeType::Standard),
            seqno: 1,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(3, crate::TreeType::Standard),
            seqno: 2,
        },
    ]);
    // CURRENT points at v1 (the snapshot the edit log is layered on).
    history.snapshot_id = 1;
    seed_version_files(dir, &history, &fs)?;

    // Watermark 3 evicts v1 (seqno 0) and v2 (seqno 1) from the history.
    history.maintenance(dir, 3, &fs)?;

    assert!(
        fs.exists(&dir.join("v1"))?,
        "the CURRENT snapshot file must NOT be GC'd even when its version is evicted"
    );
    assert!(
        !fs.exists(&dir.join("v2"))?,
        "a non-snapshot evicted version's file is still removed"
    );
    Ok(())
}

#[test]
fn super_version_gc_below_watermark_simple() -> crate::Result<()> {
    let fs = MemFs::new();
    let dir = Path::new("/gc/simple");
    let mut history = test_super_versions(vec![
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(1, crate::TreeType::Standard),
            seqno: 0,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(2, crate::TreeType::Standard),
            seqno: 1,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(3, crate::TreeType::Standard),
            seqno: 2,
        },
    ]);
    seed_version_files(dir, &history, &fs)?;

    history.maintenance(dir, 3, &fs)?;

    assert_eq!(history.len(), 1);
    // v1 and v2 deleted by GC, v3 kept
    assert!(!fs.exists(&dir.join("v1"))?);
    assert!(!fs.exists(&dir.join("v2"))?);
    assert!(fs.exists(&dir.join("v3"))?);

    Ok(())
}

#[test]
fn super_version_gc_below_watermark_simple_2() -> crate::Result<()> {
    let fs = MemFs::new();
    let dir = Path::new("/gc/simple2");
    let mut history = test_super_versions(vec![
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(1, crate::TreeType::Standard),
            seqno: 0,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(2, crate::TreeType::Standard),
            seqno: 1,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(3, crate::TreeType::Standard),
            seqno: 2,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(4, crate::TreeType::Standard),
            seqno: 8,
        },
    ]);
    seed_version_files(dir, &history, &fs)?;

    history.maintenance(dir, 3, &fs)?;

    assert_eq!(history.len(), 2);
    // v1 and v2 deleted, v3 and v4 kept
    assert!(!fs.exists(&dir.join("v1"))?);
    assert!(!fs.exists(&dir.join("v2"))?);
    assert!(fs.exists(&dir.join("v3"))?);
    assert!(fs.exists(&dir.join("v4"))?);

    Ok(())
}

#[test]
fn super_version_gc_below_watermark_keep() -> crate::Result<()> {
    let fs = MemFs::new();
    let dir = Path::new("/gc/keep");
    let mut history = test_super_versions(vec![
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(1, crate::TreeType::Standard),
            seqno: 0,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(2, crate::TreeType::Standard),
            seqno: 8,
        },
    ]);
    seed_version_files(dir, &history, &fs)?;

    history.maintenance(dir, 3, &fs)?;

    assert_eq!(history.len(), 2);
    // Both kept — no version below watermark has a successor also below watermark
    assert!(fs.exists(&dir.join("v1"))?);
    assert!(fs.exists(&dir.join("v2"))?);

    Ok(())
}

/// Three versions at seqnos 0, 2, 8 (the history is never pruned here, so
/// the front keeps the seed version at seqno 0).
fn three_versions() -> SuperVersions {
    test_super_versions(vec![
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(1, crate::TreeType::Standard),
            seqno: 0,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(2, crate::TreeType::Standard),
            seqno: 2,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(3, crate::TreeType::Standard),
            seqno: 8,
        },
    ])
}

/// The snapshot resolves to the NEWEST version installed strictly below it:
/// a snapshot equal to a version's seqno does not see that version.
#[test]
fn get_version_for_snapshot_picks_newest_version_below_seqno() -> crate::Result<()> {
    let history = three_versions();

    assert_eq!(history.get_version_for_snapshot(1)?.version.id(), 1);
    assert_eq!(history.get_version_for_snapshot(2)?.version.id(), 1);
    assert_eq!(history.get_version_for_snapshot(3)?.version.id(), 2);
    assert_eq!(history.get_version_for_snapshot(8)?.version.id(), 2);
    assert_eq!(history.get_version_for_snapshot(9)?.version.id(), 3);
    assert_eq!(
        history.get_version_for_snapshot(SeqNo::MAX)?.version.id(),
        3
    );
    Ok(())
}

/// Snapshot 0 is served from the oldest retained version, before and after
/// the front has moved: nothing is visible at 0 from any version, so the
/// read must not be refused.
#[test]
fn get_version_for_snapshot_at_zero_serves_oldest_retained() -> crate::Result<()> {
    let fs = MemFs::new();
    let dir = Path::new("/snapshot/zero");
    let mut history = three_versions();
    seed_version_files(dir, &history, &fs)?;

    assert_eq!(history.get_version_for_snapshot(0)?.version.id(), 1);

    // Watermark 5 keeps the newest version below it (seqno 2) and evicts the
    // seed; snapshot 0 now resolves to the new front instead of failing.
    history.maintenance(dir, 5, &fs)?;
    assert_eq!(history.oldest_retained_seqno(), 2);
    assert_eq!(history.get_version_for_snapshot(0)?.version.id(), 2);
    Ok(())
}

/// After pruning, a snapshot at or below the front's seqno has no version
/// to be served from: the error names both the request and the boundary,
/// and the first servable snapshot is exactly `oldest_retained + 1`.
#[test]
fn get_version_for_snapshot_below_retention_returns_error() -> crate::Result<()> {
    let fs = MemFs::new();
    let dir = Path::new("/snapshot/below");
    let mut history = three_versions();
    seed_version_files(dir, &history, &fs)?;

    history.maintenance(dir, 5, &fs)?;
    let oldest = history.oldest_retained_seqno();
    assert_eq!(oldest, 2);

    for requested in [1, oldest] {
        match history.get_version_for_snapshot(requested) {
            Err(crate::Error::SnapshotBelowRetention {
                requested: got,
                oldest_retained,
            }) => {
                assert_eq!(got, requested);
                assert_eq!(oldest_retained, oldest);
            }
            Err(other) => panic!("snapshot {requested}: wrong error {other:?}"),
            Ok(version) => panic!(
                "snapshot {requested} must be below retention, got version #{}",
                version.version.id()
            ),
        }
    }

    assert_eq!(
        history.get_version_for_snapshot(oldest + 1)?.version.id(),
        2
    );
    Ok(())
}

/// The boundary is the front's seqno: `0` while the seed version survives,
/// the retained front's seqno after each prune, the back's seqno once the
/// history is drained to the latest.
#[test]
fn oldest_retained_seqno_tracks_the_history_front() -> crate::Result<()> {
    let fs = MemFs::new();
    let dir = Path::new("/snapshot/front");
    let mut history = three_versions();
    seed_version_files(dir, &history, &fs)?;

    assert_eq!(history.oldest_retained_seqno(), 0);

    // Watermark 1 keeps everything: no version below it has a successor
    // also below it.
    history.maintenance(dir, 1, &fs)?;
    assert_eq!(history.oldest_retained_seqno(), 0);

    history.maintenance(dir, 5, &fs)?;
    assert_eq!(history.oldest_retained_seqno(), 2);

    history.drain_obsolete_to_latest();
    assert_eq!(history.oldest_retained_seqno(), 8);
    assert!(matches!(
        history.get_version_for_snapshot(8),
        Err(crate::Error::SnapshotBelowRetention {
            requested: 8,
            oldest_retained: 8
        })
    ));
    assert_eq!(history.get_version_for_snapshot(9)?.version.id(), 3);
    Ok(())
}

/// The boundary is the FRONT's seqno even when a later version carries a
/// smaller one (a reopen at a persisted floor followed by an install from a
/// reset counter): a read below the front must not be served from that
/// "newer" version just because its seqno happens to sit below the request.
#[test]
fn get_version_for_snapshot_checks_the_front_before_searching() -> crate::Result<()> {
    let history = test_super_versions(vec![
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(1, crate::TreeType::Standard),
            seqno: 10,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(2, crate::TreeType::Standard),
            seqno: 3,
        },
    ]);

    assert_eq!(history.oldest_retained_seqno(), 10);
    assert!(matches!(
        history.get_version_for_snapshot(5),
        Err(crate::Error::SnapshotBelowRetention {
            requested: 5,
            oldest_retained: 10
        })
    ));
    assert_eq!(history.get_version_for_snapshot(11)?.version.id(), 2);
    Ok(())
}

/// A history seeded from a recovered version starts at that version's
/// retention floor, so the reopened boundary is the persisted one.
#[test]
fn new_history_seeds_the_front_at_the_retention_floor() {
    let version = Version::new(7, crate::TreeType::Standard).with_retention_floor(42);
    let history = SuperVersions::new(
        version,
        &default_comparator(),
        SyncMode::Normal,
        7,
        1024 * 1024,
    );
    assert_eq!(history.oldest_retained_seqno(), 42);
    assert!(history.get_version_for_snapshot(42).is_err());
    assert!(history.get_version_for_snapshot(43).is_ok());
}

#[test]
fn super_version_gc_below_watermark_shadowed() -> crate::Result<()> {
    let fs = MemFs::new();
    let dir = Path::new("/gc/shadowed");
    let mut history = test_super_versions(vec![
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(1, crate::TreeType::Standard),
            seqno: 0,
        },
        SuperVersion {
            active_memtable: Arc::new(new_memtable(0)),
            sealed_memtables: Arc::default(),
            version: Version::new(2, crate::TreeType::Standard),
            seqno: 2,
        },
    ]);
    seed_version_files(dir, &history, &fs)?;

    history.maintenance(dir, 3, &fs)?;

    assert_eq!(history.len(), 1);
    // v1 deleted, v2 kept
    assert!(!fs.exists(&dir.join("v1"))?);
    assert!(fs.exists(&dir.join("v2"))?);

    Ok(())
}
