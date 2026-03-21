use crate::config::BenchConfig;
use crate::db::{prefill_sequential, read_seqno};
use crate::reporter::Reporter;
use crate::workloads::Workload;
use lsm_tree::{AbstractTree, AnyTree};
use std::sync::atomic::AtomicU64;
use std::time::Instant;

pub struct ReadSeq;

impl Workload for ReadSeq {
    fn name(&self) -> &'static str {
        "readseq"
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
        let mut count = 0u64;

        reporter.start();

        let iter = tree.iter(read_seq, None);
        for item in iter {
            let t = Instant::now();
            let _kv = item;
            reporter.record(t.elapsed().as_nanos() as u64);

            count += 1;
            if count >= config.num {
                break;
            }
        }

        reporter.stop();

        eprintln!("Scanned {count} entries");

        Ok(())
    }
}
