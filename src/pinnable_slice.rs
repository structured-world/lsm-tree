// Copyright (c) 2024-present, fjall-rs
// This source code is licensed under both the Apache 2.0 and MIT License
// (found in the LICENSE-* files in the repository)

//! Zero-copy value reference that can pin block cache entries.
//!
//! [`PinnableSlice`] is inspired by `RocksDB`'s `PinnableSlice`
//! (`include/rocksdb/slice.h:179-263`). It wraps a value that was read from
//! the LSM tree and indicates whether the underlying data is pinned in the
//! block cache (via an `Arc`-shared [`Block`]) or independently owned
//! (e.g. from a memtable or merge result).
//!
//! When the value comes from an on-disk data block that is in the block cache,
//! holding a `PinnableSlice::Pinned` keeps the block alive (preventing
//! eviction) for the duration of the reference. The value bytes point directly
//! into the block's decompressed data — no copy is performed.
//!
//! Memtable and blob-resolved values use the `Owned` variant.

use crate::{Slice, UserValue, table::Block};

/// A value reference that may be pinned in the block cache.
///
/// Use [`PinnableSlice::as_ref`] to access the raw bytes regardless of variant.
///
/// # Lifetime
///
/// As long as a `PinnableSlice::Pinned` is alive, the underlying
/// [`Block`] will not be evicted from the cache. Drop the `PinnableSlice`
/// when you are done with the value to allow cache eviction.
#[derive(Clone)]
pub enum PinnableSlice {
    /// Value pinned in block cache — zero copy.
    ///
    /// The [`Block`] (which is `Arc`-based) keeps the decompressed block
    /// data alive. `value` is a sub-slice of the block's data created via
    /// [`Slice::slice`], sharing the same backing allocation.
    Pinned {
        /// Keeps the block alive in the cache.
        _block: Block,
        /// Zero-copy sub-slice into the block's decompressed data.
        value: Slice,
    },

    /// Value owned independently (memtable, blob, merge result).
    Owned(UserValue),
}

impl PinnableSlice {
    /// Creates a pinned value referencing data within a block cache entry.
    #[must_use]
    pub fn pinned(block: Block, value: Slice) -> Self {
        Self::Pinned {
            _block: block,
            value,
        }
    }

    /// Creates an owned value (not pinned in any cache).
    #[must_use]
    pub fn owned(value: UserValue) -> Self {
        Self::Owned(value)
    }

    /// Returns `true` if this value is pinned in the block cache.
    #[must_use]
    pub fn is_pinned(&self) -> bool {
        matches!(self, Self::Pinned { .. })
    }

    /// Returns the raw value bytes.
    #[must_use]
    pub fn value(&self) -> &[u8] {
        self.as_ref()
    }

    /// Returns the length of the value in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.as_ref().len()
    }

    /// Returns `true` if the value is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }

    /// Converts this `PinnableSlice` into an owned `UserValue`.
    ///
    /// For the `Pinned` variant, this unpins the block and returns the
    /// value slice (which is already reference-counted via `ByteView`).
    /// For the `Owned` variant, the value is returned directly.
    #[must_use]
    pub fn into_value(self) -> UserValue {
        match self {
            Self::Pinned { value, .. } => value,
            Self::Owned(v) => v,
        }
    }
}

impl std::fmt::Debug for PinnableSlice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pinned { value, .. } => {
                f.debug_struct("Pinned").field("len", &value.len()).finish()
            }
            Self::Owned(v) => f.debug_tuple("Owned").field(&v.len()).finish(),
        }
    }
}

impl AsRef<[u8]> for PinnableSlice {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Pinned { value, .. } => value.as_ref(),
            Self::Owned(v) => v.as_ref(),
        }
    }
}

impl PartialEq<[u8]> for PinnableSlice {
    fn eq(&self, other: &[u8]) -> bool {
        self.as_ref() == other
    }
}

impl PartialEq<&[u8]> for PinnableSlice {
    fn eq(&self, other: &&[u8]) -> bool {
        self.as_ref() == *other
    }
}

impl From<PinnableSlice> for UserValue {
    fn from(ps: PinnableSlice) -> Self {
        ps.into_value()
    }
}
