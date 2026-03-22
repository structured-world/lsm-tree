// Copyright (c) 2024-present, fjall-rs
// This source code is licensed under both the Apache 2.0 and MIT License
// (found in the LICENSE-* files in the repository)

use crate::{fs::FsFile, Slice};
use std::{io::Write, path::Path};

pub const MAGIC_BYTES: [u8; 4] = [b'L', b'S', b'M', 3];

pub const TABLES_FOLDER: &str = "tables";
pub const BLOBS_FOLDER: &str = "blobs";
pub const CURRENT_VERSION_FILE: &str = "current";

/// Reads bytes from a file at the given offset without changing the cursor.
///
/// Uses [`FsFile::read_at`] (equivalent to `pread(2)`) so multiple threads
/// can call this concurrently on the same file handle.
pub fn read_exact(file: &dyn FsFile, offset: u64, size: usize) -> std::io::Result<Slice> {
    // SAFETY: This slice builder starts uninitialized, but we know its length
    //
    // We use FsFile::read_at which gives us the number of bytes read.
    // If that number does not match the slice length, the function errors,
    // so the (partially) uninitialized buffer is discarded.
    //
    // Additionally, generally, block loads furthermore do a checksum check which
    // would likely catch the buffer being wrong somehow.
    #[expect(unsafe_code, reason = "see safety")]
    let mut builder = unsafe { Slice::builder_unzeroed(size) };

    let bytes_read = file.read_at(&mut builder, offset)?;

    if bytes_read != size {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!("read_exact({bytes_read}) at {offset} did not read enough bytes {size}; file has length {}", file.metadata()?.len),
        ));
    }

    Ok(builder.freeze().into())
}

/// Atomically rewrites a file.
pub fn rewrite_atomic(path: &Path, content: &[u8]) -> std::io::Result<()> {
    #[expect(
        clippy::expect_used,
        reason = "every file should have a parent directory"
    )]
    let folder = path.parent().expect("should have a parent");

    let mut temp_file = tempfile::NamedTempFile::new_in(folder)?;
    temp_file.write_all(content)?;
    temp_file.flush()?;
    temp_file.as_file_mut().sync_all()?;
    temp_file.persist(path)?;

    // TODO: not sure why it fails on Windows...
    #[cfg(not(target_os = "windows"))]
    {
        let file = std::fs::File::open(path)?;
        file.sync_all()?;

        #[expect(
            clippy::expect_used,
            reason = "files should always have a parent directory"
        )]
        let folder = path.parent().expect("should have parent folder");
        fsync_directory(folder)?;
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn fsync_directory(path: &Path) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    debug_assert!(file.metadata()?.is_dir());
    file.sync_all()
}

#[cfg(target_os = "windows")]
pub fn fsync_directory(path: &Path) -> std::io::Result<()> {
    // Cannot fsync directory on Windows
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use test_log::test;

    #[test]
    fn read_exact_short_read_returns_error() {
        use crate::fs::{FsFile, FsMetadata};

        /// FsFile that always reads fewer bytes than requested.
        struct ShortReadFile;

        impl std::io::Read for ShortReadFile {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Ok(0)
            }
        }
        impl std::io::Write for ShortReadFile {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl std::io::Seek for ShortReadFile {
            fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
                Ok(0)
            }
        }
        impl FsFile for ShortReadFile {
            fn sync_all(&self) -> std::io::Result<()> {
                Ok(())
            }
            fn sync_data(&self) -> std::io::Result<()> {
                Ok(())
            }
            fn metadata(&self) -> std::io::Result<FsMetadata> {
                Ok(FsMetadata {
                    len: 0,
                    is_dir: false,
                    is_file: true,
                })
            }
            fn set_len(&self, _size: u64) -> std::io::Result<()> {
                Ok(())
            }
            fn read_at(&self, _buf: &mut [u8], _offset: u64) -> std::io::Result<usize> {
                Ok(0) // always returns 0 bytes
            }
            fn lock_exclusive(&self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let err = read_exact(&ShortReadFile, 0, 10).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
        let msg = err.to_string();
        assert!(
            msg.contains("did not read enough bytes"),
            "unexpected error message: {msg}",
        );
    }

    #[test]
    fn atomic_rewrite() -> crate::Result<()> {
        let dir = tempfile::tempdir()?;

        let path = dir.path().join("test.txt");
        {
            let mut file = File::create(&path)?;
            write!(file, "asdasdasdasdasd")?;
        }

        rewrite_atomic(&path, b"newcontent")?;

        let content = std::fs::read_to_string(&path)?;
        assert_eq!("newcontent", content);

        Ok(())
    }
}
