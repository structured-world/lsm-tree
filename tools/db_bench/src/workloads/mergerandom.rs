use crate::config::BenchConfig;
use crate::db::{make_sequential_key, make_value};
use crate::reporter::Reporter;
use crate::workloads::Workload;
use lsm_tree::{AbstractTree, AnyTree};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Writes to a small set of "hot" keys repeatedly, flushing periodically
/// to create overlapping SSTs that stress the compaction merge path.
///
/// NOTE: This uses `insert()` (full overwrites), not merge operands —
/// lsm-tree's merge operator API is internal to compaction. The workload
/// exercises the SST-level k-way merge during major compaction, which is
/// the primary merge cost path. True merge-operand benchmarks will be
/// added when the public merge API is available (CoordiNode posting lists).
pub struct MergeRandom;

impl Workload for MergeRandom {
    fn run(
        &self,
        tree: &AnyTree,
        config: &BenchConfig,
        seqno: &AtomicU64,
        reporter: &mut Reporter,
    ) -> lsm_tree::Result<()> {
        // Use a small hot key set (1024 keys) to create heavy overlap.
        let hot_keys: u64 = 1024;
        let flush_interval: u64 = 5_000;

        // 1024 hot keys need at least 2 bytes of key space (2^16 = 65536 > 1024).
        if config.key_size < 2 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "mergerandom requires --key-size >= 2 to preserve hot-key distinctness",
            )
            .into());
        }

        reporter.start();

        for i in 0..config.num {
            let key_idx = i % hot_keys;
            let key = make_sequential_key(key_idx, config.key_size);
            let value = make_value(config.value_size);
            let seq = seqno.fetch_add(1, Ordering::Relaxed);

            let t = Instant::now();
            tree.insert(key, value, seq);
            reporter.record_duration(t.elapsed());

            if (i + 1) % flush_interval == 0 {
                let t_flush = Instant::now();
                tree.flush_active_memtable(0)?;
                reporter.record_duration(t_flush.elapsed());
            }
        }

        // Final flush + major compaction to exercise merge.
        let t_flush = Instant::now();
        tree.flush_active_memtable(0)?;
        reporter.record_duration(t_flush.elapsed());

        let compact_seqno = seqno.load(Ordering::Relaxed);
        let t_compact = Instant::now();
        tree.major_compact(64 * 1024 * 1024, compact_seqno)?;
        reporter.record_duration(t_compact.elapsed());

        reporter.stop();

        eprintln!(
            "Merged {} writes over {} hot keys, {} tables after compaction",
            config.num,
            hot_keys,
            tree.table_count(),
        );

        Ok(())
    }
}
