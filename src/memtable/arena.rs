// Copyright (c) 2024-present, fjall-rs
// This source code is licensed under both the Apache 2.0 and MIT License
// (found in the LICENSE-* files in the repository)

//! Bump-allocating arena for skiplist node storage.
//!
//! All node data (metadata, keys, values, tower pointers) is allocated from a
//! single contiguous byte buffer via an atomic bump pointer.  This gives better
//! cache locality than per-node heap allocation and enables O(1) bulk
//! deallocation when the memtable is dropped.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU32, Ordering};

/// A bump-allocating arena backed by a fixed-size byte buffer.
///
/// Thread-safe: concurrent allocations are serialised by a CAS loop on the
/// bump pointer.  Once allocated, regions are immutable (written once during
/// node construction, then read concurrently).  Tower entries are the exception
/// — they are accessed through [`AtomicU32`] references obtained via
/// [`get_atomic_u32`](Self::get_atomic_u32).
pub(crate) struct Arena {
    /// The backing buffer.  Accessed through `UnsafeCell` because:
    /// - Allocation writes to a region that no other thread can reach (the
    ///   allocating thread owns the just-bumped range).
    /// - Reads only happen after a release/acquire pair on the bump pointer.
    buf: UnsafeCell<Box<[u8]>>,

    /// Next free byte offset.  Starts at 1 (offset 0 is the UNSET sentinel).
    offset: AtomicU32,
}

// SAFETY: All mutable access to `buf` is to non-overlapping, freshly-allocated
// regions protected by the atomic bump pointer.  Concurrent reads use
// acquire/release ordering via `offset`.
unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

impl Arena {
    /// Creates a new arena with the given capacity in bytes.
    pub fn new(capacity: u32) -> Self {
        Self {
            buf: UnsafeCell::new(vec![0u8; capacity as usize].into_boxed_slice()),
            // Reserve offset 0 as the UNSET sentinel.
            offset: AtomicU32::new(1),
        }
    }

    /// Returns the total capacity of the arena in bytes.
    pub fn capacity(&self) -> u32 {
        // SAFETY: reading `.len()` is a read-only operation on the boxed slice
        // and does not conflict with concurrent writes to different regions.
        // SAFETY: reading `.len()` is a read-only operation on the boxed slice
        // and does not conflict with concurrent writes to different regions.
        unsafe { (&*self.buf.get()).len() as u32 }
    }

    /// Allocates `size` bytes with the given alignment.
    ///
    /// Returns the start offset of the allocated region, or `None` if the
    /// arena is exhausted.  `align` **must** be a power of two.
    pub fn alloc(&self, size: u32, align: u32) -> Option<u32> {
        debug_assert!(align.is_power_of_two());

        let cap = self.capacity();

        loop {
            let cur = self.offset.load(Ordering::Relaxed);
            let aligned = (cur + align - 1) & !(align - 1);
            let new_end = aligned.checked_add(size)?;

            if new_end > cap {
                return None;
            }

            match self.offset.compare_exchange_weak(
                cur,
                new_end,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(aligned),
                Err(_) => continue,
            }
        }
    }

    /// Returns a shared reference to `len` bytes starting at `offset`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `offset..offset+len` was previously
    /// allocated by this arena and that initial writes to the region have
    /// been completed (with appropriate memory ordering).
    pub unsafe fn get_bytes(&self, offset: u32, len: u32) -> &[u8] {
        let buf = &*self.buf.get();
        &buf[offset as usize..offset as usize + len as usize]
    }

    /// Returns an exclusive reference to `len` bytes starting at `offset`.
    ///
    /// # Safety
    ///
    /// The caller must ensure exclusive access to the given range (typically
    /// right after allocation, before publishing the node offset to other
    /// threads).
    pub unsafe fn get_bytes_mut(&self, offset: u32, len: u32) -> &mut [u8] {
        let buf = &mut *self.buf.get();
        &mut buf[offset as usize..offset as usize + len as usize]
    }

    /// Interprets 4 bytes at `offset` as an [`AtomicU32`] reference.
    ///
    /// # Safety
    ///
    /// - `offset` must be 4-byte aligned.
    /// - The region `[offset, offset+4)` must have been previously allocated.
    /// - No `&mut` reference to the same 4 bytes may exist concurrently.
    pub unsafe fn get_atomic_u32(&self, offset: u32) -> &AtomicU32 {
        let buf = &*self.buf.get();
        let ptr = buf.as_ptr().add(offset as usize) as *mut u32;
        debug_assert!(ptr.is_aligned(), "AtomicU32 requires 4-byte alignment");
        AtomicU32::from_ptr(ptr)
    }

    /// Total bytes allocated so far (including the reserved sentinel byte).
    #[cfg(test)]
    pub fn allocated(&self) -> u32 {
        self.offset.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_alloc_and_read() {
        let arena = Arena::new(1024);

        let off = arena.alloc(4, 4).expect("should succeed");
        assert!(off >= 1); // 0 is reserved
        assert_eq!(off % 4, 0); // aligned

        unsafe {
            let bytes = arena.get_bytes_mut(off, 4);
            bytes.copy_from_slice(&[1, 2, 3, 4]);
        }

        let read = unsafe { arena.get_bytes(off, 4) };
        assert_eq!(read, &[1, 2, 3, 4]);
    }

    #[test]
    fn alloc_respects_alignment() {
        let arena = Arena::new(256);

        // Allocate 1 byte (unaligned)
        let a = arena.alloc(1, 1).expect("ok");
        // Allocate 4 bytes with 4-byte alignment
        let b = arena.alloc(4, 4).expect("ok");
        assert_eq!(b % 4, 0);
        assert!(b > a);
    }

    #[test]
    fn alloc_returns_none_on_exhaustion() {
        let arena = Arena::new(16);

        let _ = arena.alloc(12, 1); // use most of the space
        assert!(arena.alloc(16, 1).is_none());
    }

    #[test]
    fn atomic_u32_round_trip() {
        let arena = Arena::new(64);
        let off = arena.alloc(4, 4).expect("ok");

        unsafe {
            let atom = arena.get_atomic_u32(off);
            atom.store(42, Ordering::Relaxed);
            assert_eq!(atom.load(Ordering::Relaxed), 42);
        }
    }

    #[test]
    fn concurrent_alloc() {
        use std::sync::Arc;

        let arena = Arc::new(Arena::new(1024 * 1024));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let arena = Arc::clone(&arena);
                std::thread::spawn(move || {
                    let mut offsets = Vec::new();
                    for _ in 0..1000 {
                        if let Some(off) = arena.alloc(64, 4) {
                            offsets.push(off);
                        }
                    }
                    offsets
                })
            })
            .collect();

        let mut all_offsets: Vec<u32> = Vec::new();
        for h in handles {
            all_offsets.extend(h.join().expect("thread ok"));
        }

        // All offsets must be unique and non-overlapping
        all_offsets.sort();
        for pair in all_offsets.windows(2) {
            assert!(pair[1] >= pair[0] + 64, "allocations must not overlap");
        }
    }
}
