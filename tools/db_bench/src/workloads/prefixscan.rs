use crate::config::BenchConfig;
use crate::db::{prefill_prefix_keys, read_seqno};
use crate::reporter::Reporter;
use crate::workloads::Workload;
use lsm_tree::{AbstractTree, AnyTree};
use rand::Rng;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

pub struct PrefixScan;

const NUM_PREFIXES: u16 = 256;
const SCAN_LIMIT: usize = 10;

impl Workload for PrefixScan {
    fn run(
        &self,
        tree: &AnyTree,
        config: &BenchConfig,
        seqno: &AtomicU64,
        reporter: &mut Reporter,
    ) -> lsm_tree::Result<()> {
        // Prefill with structured prefix keys.
        prefill_prefix_keys(tree, config, seqno, NUM_PREFIXES)?;

        let read_seq = read_seqno(seqno);
        let mut rng = rand::rng();

        reporter.start();

        for _ in 0..config.num {
            let prefix_idx: u16 = rng.random_range(0..NUM_PREFIXES);
            let prefix_bytes = prefix_idx.to_be_bytes();

            let t = Instant::now();
            let iter = tree.prefix(&prefix_bytes, read_seq, None);
            iter.take(SCAN_LIMIT).for_each(|_| {});
            reporter.record(t.elapsed().as_nanos() as u64);
        }

        reporter.stop();
        Ok(())
    }
}
