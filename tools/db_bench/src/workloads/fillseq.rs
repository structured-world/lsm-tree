use crate::config::BenchConfig;
use crate::db::{make_sequential_key, make_value};
use crate::reporter::Reporter;
use crate::workloads::Workload;
use lsm_tree::{AbstractTree, AnyTree};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub struct FillSeq;

impl Workload for FillSeq {
    fn name(&self) -> &'static str {
        "fillseq"
    }

    fn run(
        &self,
        tree: &AnyTree,
        config: &BenchConfig,
        seqno: &AtomicU64,
        reporter: &mut Reporter,
    ) -> lsm_tree::Result<()> {
        reporter.start();

        for i in 0..config.num {
            let key = make_sequential_key(i, config.key_size);
            let value = make_value(config.value_size);
            let seq = seqno.fetch_add(1, Ordering::Relaxed);

            let t = Instant::now();
            tree.insert(key, value, seq);
            reporter.record(t.elapsed().as_nanos() as u64);
        }

        reporter.stop();
        Ok(())
    }
}
