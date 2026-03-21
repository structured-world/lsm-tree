use crate::config::BenchConfig;
use crate::db::{make_sequential_key, prefill_sequential, read_seqno};
use crate::reporter::Reporter;
use crate::workloads::Workload;
use lsm_tree::{AbstractTree, AnyTree};
use rand::Rng;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

pub struct ReadRandom;

impl Workload for ReadRandom {
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
        let mut found = 0u64;

        // Pre-generate keys so per-iteration allocation is not included in read latency.
        let keys: Vec<Vec<u8>> = (0..config.num)
            .map(|i| make_sequential_key(i, config.key_size))
            .collect();

        reporter.start();

        for _ in 0..config.num {
            let idx: u64 = rng.random_range(0..config.num);
            let key = &keys[idx as usize];

            let t = Instant::now();
            let result = tree.get(key, read_seq)?;
            reporter.record_duration(t.elapsed());

            if result.is_some() {
                found += 1;
            }
        }

        reporter.stop();

        if config.num > 0 {
            let hit_rate = found as f64 / config.num as f64 * 100.0;
            eprintln!("Hit rate: {found}/{} ({hit_rate:.1}%)", config.num);
        }

        Ok(())
    }
}
