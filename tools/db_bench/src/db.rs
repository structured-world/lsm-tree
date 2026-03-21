use crate::config::BenchConfig;
use lsm_tree::{AbstractTree, AnyTree, SeqNo};
use std::sync::atomic::{AtomicU64, Ordering};

/// Prefill a tree with sequential keys for read benchmarks.
///
/// Returns the next available seqno.
pub fn prefill_sequential(
    tree: &AnyTree,
    config: &BenchConfig,
    seqno: &AtomicU64,
) -> lsm_tree::Result<()> {
    let batch_size = 10_000u64;

    for i in 0..config.num {
        let key = make_sequential_key(i, config.key_size);
        let value = make_value(config.value_size);
        let seq = seqno.fetch_add(1, Ordering::Relaxed);
        tree.insert(key, value, seq);

        // Flush every batch_size ops to build SSTs on disk.
        if (i + 1) % batch_size == 0 {
            tree.flush_active_memtable(0)?;
        }
    }

    // Final flush.
    tree.flush_active_memtable(0)?;

    eprintln!(
        "Prefilled {} keys ({} bytes/entry), {} tables on disk",
        config.num,
        config.entry_size(),
        tree.table_count(),
    );

    Ok(())
}

/// Prefill a tree with structured prefix keys for prefix scan benchmarks.
///
/// Keys have the format: `{prefix_byte}:{suffix_bytes}`.
pub fn prefill_prefix_keys(
    tree: &AnyTree,
    config: &BenchConfig,
    seqno: &AtomicU64,
    num_prefixes: u16,
) -> lsm_tree::Result<()> {
    let keys_per_prefix = config.num / num_prefixes as u64;
    let batch_size = 10_000u64;
    let mut total = 0u64;

    for prefix in 0..num_prefixes {
        let prefix_bytes = prefix.to_be_bytes();
        for suffix in 0..keys_per_prefix {
            let mut key = Vec::with_capacity(config.key_size);
            key.extend_from_slice(&prefix_bytes);
            let suffix_bytes = suffix.to_be_bytes();
            key.extend_from_slice(&suffix_bytes);
            key.resize(config.key_size, 0);

            let value = make_value(config.value_size);
            let seq = seqno.fetch_add(1, Ordering::Relaxed);
            tree.insert(key, value, seq);

            total += 1;
            if total % batch_size == 0 {
                tree.flush_active_memtable(0)?;
            }
        }
    }

    tree.flush_active_memtable(0)?;

    eprintln!(
        "Prefilled {} keys across {} prefixes, {} tables on disk",
        total,
        num_prefixes,
        tree.table_count(),
    );

    Ok(())
}

/// Create a sequential key from a u64 index, padded to key_size.
#[inline]
pub fn make_sequential_key(index: u64, key_size: usize) -> Vec<u8> {
    let mut key = Vec::with_capacity(key_size);
    key.extend_from_slice(&index.to_be_bytes());
    key.resize(key_size, 0);
    key
}

/// Create a random key of the given size.
#[inline]
pub fn make_random_key(key_size: usize) -> Vec<u8> {
    use rand::Rng;
    let mut key = vec![0u8; key_size];
    rand::rng().fill(&mut key[..]);
    key
}

/// Create a deterministic value of the given size.
#[inline]
pub fn make_value(value_size: usize) -> Vec<u8> {
    vec![0x42u8; value_size]
}

/// Read the current seqno for point reads (must see all prefilled data).
pub fn read_seqno(seqno: &AtomicU64) -> SeqNo {
    seqno.load(Ordering::Relaxed)
}
