use super::*;
use crate::fs::StdFs;
use test_log::test;

fn store() -> (tempfile::TempDir, Arc<dyn Fs>, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let fs: Arc<dyn Fs> = Arc::new(StdFs);
    let folder = dir.path().join(crate::file::DICTS_FOLDER);
    (dir, fs, folder)
}

#[test]
fn a_written_dictionary_reads_back_byte_for_byte() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    let dict = ZstdDictionary::new(b"representative content for a dictionary");

    write(&*fs, &folder, &dict, SyncMode::Normal)?;
    let read = read_one(&*fs, &folder, dict.id())?;

    assert_eq!(read.raw(), dict.raw());
    assert_eq!(read.id(), dict.id());
    Ok(())
}

#[test]
fn the_folder_is_created_on_the_first_write() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    assert!(
        !fs.exists(&folder)?,
        "no folder before the first dictionary"
    );

    write(
        &*fs,
        &folder,
        &ZstdDictionary::new(b"content"),
        SyncMode::Normal,
    )?;

    assert!(fs.exists(&folder)?);
    Ok(())
}

#[test]
fn writing_an_id_already_held_is_a_no_op() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    let dict = ZstdDictionary::new(b"content");

    write(&*fs, &folder, &dict, SyncMode::Normal)?;
    write(&*fs, &folder, &dict, SyncMode::Normal)?;

    assert_eq!(read_one(&*fs, &folder, dict.id())?.raw(), dict.raw());
    Ok(())
}

#[test]
fn reading_a_dictionary_the_tree_does_not_hold_reports_not_found() {
    let (_dir, fs, folder) = store();

    let err = read_one(&*fs, &folder, 12345).unwrap_err();

    // The caller distinguishes "never registered" from "corrupt", so this must
    // not surface as a mismatch.
    match err {
        crate::Error::Io(e) => assert_eq!(e.kind(), crate::io::ErrorKind::NotFound),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn a_flipped_bit_is_caught_because_the_name_is_the_digest() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    let dict = ZstdDictionary::new(b"content that will be corrupted on disk");
    write(&*fs, &folder, &dict, SyncMode::Normal)?;

    // Corrupt one byte under the live name. A silently altered dictionary is
    // the worst failure this store can have: every block written against it
    // would decompress to plausible garbage instead of failing, so the read
    // must refuse rather than hand the bytes back.
    let path = folder.join(dict.id().to_string());
    let mut raw = std::fs::read(&path).unwrap();
    *raw.first_mut().unwrap() ^= 0x01;
    std::fs::write(&path, &raw).unwrap();

    let err = read_one(&*fs, &folder, dict.id()).unwrap_err();
    match err {
        crate::Error::ZstdDictMismatch { expected, got } => {
            assert_eq!(expected, dict.id());
            assert_ne!(got, Some(dict.id()), "the corrupted bytes hash elsewhere");
        }
        other => panic!("expected ZstdDictMismatch, got {other:?}"),
    }
    Ok(())
}

#[test]
fn a_truncated_dictionary_is_caught_the_same_way() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    let dict = ZstdDictionary::new(b"content long enough to truncate");
    write(&*fs, &folder, &dict, SyncMode::Normal)?;

    let path = folder.join(dict.id().to_string());
    let raw = std::fs::read(&path).unwrap();
    let half = raw.get(..raw.len() / 2).unwrap();
    std::fs::write(&path, half).unwrap();

    assert!(matches!(
        read_one(&*fs, &folder, dict.id()),
        Err(crate::Error::ZstdDictMismatch { .. }),
    ));
    Ok(())
}

#[test]
fn the_scan_loads_every_dictionary_the_folder_holds() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    let a = ZstdDictionary::new(b"aaaaaaaaaaaaaaaaaaaa");
    let b = ZstdDictionary::new(b"bbbbbbbbbbbbbbbbbbbb");
    write(&*fs, &folder, &a, SyncMode::Normal)?;
    write(&*fs, &folder, &b, SyncMode::Normal)?;

    let set = read_all(&*fs, &folder)?;

    assert_eq!(set.len(), 2);
    assert_eq!(
        set.get(a.id()).map(|d| d.raw().to_vec()),
        Some(a.raw().to_vec())
    );
    assert_eq!(
        set.get(b.id()).map(|d| d.raw().to_vec()),
        Some(b.raw().to_vec())
    );
    Ok(())
}

#[test]
fn the_scan_of_an_empty_folder_is_an_empty_set() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    assert!(read_all(&*fs, &folder)?.is_empty());

    fs.create_dir_all(&folder)?;
    assert!(read_all(&*fs, &folder)?.is_empty());
    Ok(())
}

#[test]
fn the_scan_fails_on_a_corrupt_dictionary_rather_than_skipping_it() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    let good = ZstdDictionary::new(b"aaaaaaaaaaaaaaaaaaaa");
    let bad = ZstdDictionary::new(b"bbbbbbbbbbbbbbbbbbbb");
    write(&*fs, &folder, &good, SyncMode::Normal)?;
    write(&*fs, &folder, &bad, SyncMode::Normal)?;

    let path = folder.join(bad.id().to_string());
    let mut raw = std::fs::read(&path).unwrap();
    *raw.first_mut().unwrap() ^= 0x01;
    std::fs::write(&path, &raw).unwrap();

    // Skipping it would turn a detectable corruption into "unknown dictionary
    // id" on the first table that needs it, far from the cause.
    assert!(read_all(&*fs, &folder).is_err());
    Ok(())
}

#[test]
fn the_scan_ignores_files_the_engine_does_not_own() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    let dict = ZstdDictionary::new(b"content");
    write(&*fs, &folder, &dict, SyncMode::Normal)?;
    std::fs::write(folder.join("notes.txt"), b"mine").unwrap();
    std::fs::write(folder.join("7.tmp"), b"unpublished").unwrap();

    let set = read_all(&*fs, &folder)?;

    assert_eq!(set.len(), 1, "only the published dictionary is loaded");
    assert!(set.get(dict.id()).is_some());
    Ok(())
}

#[test]
fn removing_a_dictionary_leaves_the_others() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    let a = ZstdDictionary::new(b"aaaaaaaaaaaaaaaaaaaa");
    let b = ZstdDictionary::new(b"bbbbbbbbbbbbbbbbbbbb");
    write(&*fs, &folder, &a, SyncMode::Normal)?;
    write(&*fs, &folder, &b, SyncMode::Normal)?;

    remove(&*fs, &folder, a.id(), SyncMode::Normal)?;

    assert!(read_one(&*fs, &folder, a.id()).is_err());
    assert_eq!(read_one(&*fs, &folder, b.id())?.raw(), b.raw());
    Ok(())
}

#[test]
fn removing_an_absent_dictionary_succeeds() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    fs.create_dir_all(&folder)?;

    remove(&*fs, &folder, 4242, SyncMode::Normal)?;
    Ok(())
}

#[test]
fn the_sweep_takes_temps_and_leaves_everything_else() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    let dict = ZstdDictionary::new(b"content");
    write(&*fs, &folder, &dict, SyncMode::Normal)?;

    // A crashed registration leaves this behind.
    let temp = folder.join(format!("{}{DICT_TMP_SUFFIX}", 777));
    std::fs::write(&temp, b"half-written").unwrap();
    // An operator's file, which the engine does not own and must not touch.
    let foreign = folder.join("notes.txt");
    std::fs::write(&foreign, b"mine").unwrap();

    sweep_temps(&*fs, &folder)?;

    assert!(!fs.exists(&temp)?, "the unpublished temp is disposable");
    assert!(fs.exists(&foreign)?, "a foreign name is never swept");
    assert_eq!(
        read_one(&*fs, &folder, dict.id())?.raw(),
        dict.raw(),
        "a published dictionary survives the sweep",
    );
    Ok(())
}

#[test]
fn sweeping_a_folder_that_does_not_exist_succeeds() -> crate::Result<()> {
    let (_dir, fs, folder) = store();
    sweep_temps(&*fs, &folder)?;
    Ok(())
}
