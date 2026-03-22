// Copyright (c) 2024-present, fjall-rs
// This source code is licensed under both the Apache 2.0 and MIT License
// (found in the LICENSE-* files in the repository)

//! Multi-block bump-allocating arena for skiplist node storage.
//!
//! Blocks are allocated lazily in 4 MiB chunks — the arena never pre-allocates
//! a large contiguous buffer, so it works on 32-bit targets with limited
//! address space.  Once a block is full, a new one is allocated and the
//! remaining space in the old block is abandoned (waste is negligible for
//! typical node allocations of < 100 bytes).

use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

/// Bits used for the within-block offset.  2^22 = 4 MiB per block.
const BLOCK_SHIFT: u32 = 22;

/// Size of each arena block in bytes (4 MiB).
const BLOCK_SIZE: u32 = 1 << BLOCK_SHIFT;

/// Bitmask for extracting the within-block offset from an encoded u32.
const BLOCK_MASK: u32 = BLOCK_SIZE - 1;

/// Maximum number of blocks.  With 4 MiB blocks and 10-bit index this
/// supports up to 4 GiB total arena capacity.
const MAX_BLOCKS: usize = 1 << (32 - BLOCK_SHIFT); // 1024

/// A multi-block bump-allocating arena.
///
/// Thread-safe: concurrent allocations are serialised by a CAS loop on the
/// bump cursor.  Blocks are allocated lazily via CAS on `AtomicPtr`, so only
/// the blocks that are actually needed consume memory.
///
/// The u32 offset returned by [`alloc`](Self::alloc) encodes both the block
/// index (high 10 bits) and the within-block offset (low 22 bits).
pub struct Arena {
    /// Block pointers.  Null means not yet allocated.
    blocks: Box<[AtomicPtr<u8>]>,

    /// Allocation cursor.  High 10 bits = block index, low 22 bits = offset
    /// within that block.  Starts at 1 (offset 0 is the UNSET sentinel).
    cursor: AtomicU32,
}

// SAFETY: AtomicPtr and AtomicU32 are Send+Sync.  Block data is accessed via
// bump allocation (non-overlapping regions) and subsequent immutable reads.
unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

impl Arena {
    /// Creates a new empty arena.  No memory is allocated until the first
    /// [`alloc`](Self::alloc) call.
    pub fn new() -> Self {
        // Allocate the block-pointer array.  All entries start as null.
        let mut blocks = Vec::with_capacity(MAX_BLOCKS);
        for _ in 0..MAX_BLOCKS {
            blocks.push(AtomicPtr::new(ptr::null_mut()));
        }

        Self {
            blocks: blocks.into_boxed_slice(),
            // Offset 0 is reserved as the UNSET sentinel.
            cursor: AtomicU32::new(1),
        }
    }

    /// Allocates `size` bytes with the given alignment.
    ///
    /// Returns the encoded offset, or `None` if the arena is exhausted
    /// (> 4 GiB total).  `align` **must** be a power of two.
    pub fn alloc(&self, size: u32, align: u32) -> Option<u32> {
        if !align.is_power_of_two() || size == 0 {
            return None;
        }

        loop {
            let cur = self.cursor.load(Ordering::Relaxed);
            let block_idx = cur >> BLOCK_SHIFT;
            let offset = cur & BLOCK_MASK;
            let aligned = (offset + align - 1) & !(align - 1);

            if let Some(new_end) = aligned.checked_add(size) {
                if new_end <= BLOCK_SIZE {
                    // Fits in the current block.
                    let new_cursor = (block_idx << BLOCK_SHIFT) | new_end;
                    if self
                        .cursor
                        .compare_exchange_weak(cur, new_cursor, Ordering::AcqRel, Ordering::Relaxed)
                        .is_ok()
                    {
                        self.ensure_block(block_idx as usize);
                        return Some((block_idx << BLOCK_SHIFT) | aligned);
                    }
                    // CAS failed — retry.
                } else {
                    // Doesn't fit — advance to the next block.
                    let new_block = block_idx + 1;
                    if new_block as usize >= MAX_BLOCKS {
                        return None;
                    }
                    let new_cursor = new_block << BLOCK_SHIFT;
                    // Try to advance; if another thread already did, we'll just
                    // retry and pick up the updated cursor.
                    let _ = self.cursor.compare_exchange_weak(
                        cur,
                        new_cursor,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    );
                }
            } else {
                // size overflow — shouldn't happen with realistic values.
                return None;
            }
        }
    }

    /// Returns a shared reference to `len` bytes at the encoded `offset`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `offset..offset+len` was previously
    /// allocated by this arena and fully initialised.
    pub unsafe fn get_bytes(&self, offset: u32, len: u32) -> &[u8] {
        let (ptr, off) = self.decode(offset);
        // SAFETY: caller guarantees the range is allocated and initialised.
        std::slice::from_raw_parts(ptr.add(off), len as usize)
    }

    /// Returns an exclusive reference to `len` bytes at the encoded `offset`.
    ///
    /// # Safety
    ///
    /// The caller must ensure exclusive access to the given range.
    #[expect(
        clippy::mut_from_ref,
        reason = "interior mutability by design; caller guarantees exclusive access"
    )]
    pub unsafe fn get_bytes_mut(&self, offset: u32, len: u32) -> &mut [u8] {
        let (ptr, off) = self.decode(offset);
        // SAFETY: caller guarantees exclusive access (typically right after alloc,
        // before the node offset is published to other threads).
        std::slice::from_raw_parts_mut(ptr.add(off), len as usize)
    }

    /// Interprets 4 bytes at `offset` as an [`AtomicU32`] reference.
    ///
    /// # Safety
    ///
    /// - `offset` must be 4-byte aligned.
    /// - The region `[offset, offset+4)` must have been previously allocated.
    /// - No `&mut` reference to the same 4 bytes may exist concurrently.
    pub unsafe fn get_atomic_u32(&self, offset: u32) -> &AtomicU32 {
        let (ptr, off) = self.decode(offset);
        // SAFETY: caller guarantees alignment and prior allocation.
        // Global allocator alignment (>= 4 on all targets) ensures the block
        // base is aligned; alloc() with align=4 ensures the within-block
        // offset is aligned.
        #[expect(
            clippy::cast_ptr_alignment,
            reason = "caller guarantees 4-byte alignment via alloc(..., 4)"
        )]
        let atom_ptr = ptr.add(off).cast::<u32>();
        debug_assert!(atom_ptr.is_aligned(), "AtomicU32 requires 4-byte alignment");
        AtomicU32::from_ptr(atom_ptr)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Decodes an encoded offset into a `(block_base_ptr, within_block_offset)`.
    ///
    /// # Safety (caller implied)
    ///
    /// The offset must have been returned by a prior `alloc()` call, which
    /// guarantees the block is allocated and the offset is in-bounds.
    #[expect(
        clippy::indexing_slicing,
        reason = "block_idx < MAX_BLOCKS by construction (alloc enforces this)"
    )]
    unsafe fn decode(&self, offset: u32) -> (*mut u8, usize) {
        let block_idx = (offset >> BLOCK_SHIFT) as usize;
        let off = (offset & BLOCK_MASK) as usize;
        let ptr = self.blocks[block_idx].load(Ordering::Acquire);
        debug_assert!(!ptr.is_null(), "accessing unallocated arena block");
        (ptr, off)
    }

    /// Ensures that the block at `idx` is allocated.  Uses CAS to avoid
    /// double-allocation when multiple threads race.
    #[expect(
        clippy::indexing_slicing,
        reason = "idx < MAX_BLOCKS enforced by alloc()"
    )]
    fn ensure_block(&self, idx: usize) {
        if self.blocks[idx].load(Ordering::Acquire).is_null() {
            // Allocate a new zero-initialised block.
            let block = vec![0u8; BLOCK_SIZE as usize].into_boxed_slice();
            let raw = Box::into_raw(block).cast::<u8>();

            // CAS null → raw.  If another thread won, free our block.
            if self.blocks[idx]
                .compare_exchange(ptr::null_mut(), raw, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                // Another thread already allocated this block — drop ours.
                unsafe {
                    drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
                        raw,
                        BLOCK_SIZE as usize,
                    )));
                }
            }
        }
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        for block in &*self.blocks {
            let ptr = block.load(Ordering::Relaxed);
            if !ptr.is_null() {
                unsafe {
                    drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
                        ptr,
                        BLOCK_SIZE as usize,
                    )));
                }
            }
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests use expect for brevity")]
mod tests {
    use super::*;

    #[test]
    fn basic_alloc_and_read() {
        let arena = Arena::new();

        let off = arena.alloc(4, 4).expect("should succeed");
        assert!(off >= 1); // 0 is reserved
        assert_eq!(off & 3, 0); // 4-byte aligned

        // SAFETY: `off` was just allocated with size 4; exclusive access.
        unsafe {
            let bytes = arena.get_bytes_mut(off, 4);
            bytes.copy_from_slice(&[1, 2, 3, 4]);
        }

        let read = unsafe { arena.get_bytes(off, 4) };
        assert_eq!(read, &[1, 2, 3, 4]);
    }

    #[test]
    fn alloc_respects_alignment() {
        let arena = Arena::new();

        let a = arena.alloc(1, 1).expect("ok");
        let b = arena.alloc(4, 4).expect("ok");
        assert_eq!(b & 3, 0); // 4-byte aligned
        assert!(b > a);
    }

    #[test]
    fn alloc_crosses_block_boundary() {
        let arena = Arena::new();

        // Fill most of the first block.
        let big = BLOCK_SIZE - 64;
        let off1 = arena.alloc(big, 1).expect("ok");
        assert_eq!(off1 >> BLOCK_SHIFT, 0); // block 0

        // This allocation should spill into block 1.
        let off2 = arena.alloc(128, 4).expect("ok");
        assert_eq!(off2 >> BLOCK_SHIFT, 1); // block 1
    }

    #[test]
    fn atomic_u32_round_trip() {
        let arena = Arena::new();
        let off = arena.alloc(4, 4).expect("ok");

        // SAFETY: freshly allocated, 4-byte aligned.
        unsafe {
            let atom = arena.get_atomic_u32(off);
            atom.store(42, Ordering::Relaxed);
            assert_eq!(atom.load(Ordering::Relaxed), 42);
        }
    }

    #[test]
    fn concurrent_alloc() {
        use std::sync::Arc;

        let arena = Arc::new(Arena::new());
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

        // All offsets must be unique (no overlapping allocations).
        all_offsets.sort();
        all_offsets.dedup();
        assert_eq!(all_offsets.len(), 8000);
    }
}
