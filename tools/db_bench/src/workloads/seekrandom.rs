use crate::config::BenchConfig;
use crate::db::{make_sequential_key, prefill_sequential, read_seqno};
use crate::reporter::Reporter;
use crate::workloads::Workload;
use lsm_tree::{AbstractTree, AnyTree};
use rand::Rng;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

pub struct SeekRandom;

impl Workload for SeekRandom {
    fn name(&self) -> &'static str {
        "seekrandom"
    }

    fn run(
        &self,
        tree: &AnyTree,
        config: &BenchConfig,
        seqno: &AtomicU64,
        reporter: &mut Reporter,
    ) -> lsm_tree::Result<()> {
        // Prefill the tree with sequential keys.
        prefill_sequential(tree, config, seqno)?;

        let read_seq = read_seqno(seqno);
        let mut rng = rand::rng();

        reporter.start();

        for _ in 0..config.num {
            let idx: u64 = rng.random_range(0..config.num);
            let key = make_sequential_key(idx, config.key_size);

            let t = Instant::now();
            // Seek to key and read the next entry.
            let mut iter = tree.range(key.., read_seq, None);
            let _next = iter.next();
            reporter.record(t.elapsed().as_nanos() as u64);
        }

        reporter.stop();
        Ok(())
    }
}
