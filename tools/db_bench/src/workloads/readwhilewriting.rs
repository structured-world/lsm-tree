use crate::config::BenchConfig;
use crate::db::{make_random_key, make_sequential_key, make_value, prefill_sequential, read_seqno};
use crate::reporter::Reporter;
use crate::workloads::Workload;
use lsm_tree::{AbstractTree, AnyTree};
use rand::Rng;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Barrier;
use std::time::Instant;

pub struct ReadWhileWriting;

impl Workload for ReadWhileWriting {
    fn name(&self) -> &'static str {
        "readwhilewriting"
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

        let threads = config.threads.max(2);
        let reader_count = threads - 1;
        let ops_per_reader = config.num / reader_count as u64;
        let barrier = Barrier::new(threads);

        reporter.start();

        std::thread::scope(|s| {
            // Spawn reader threads — borrow barrier by reference.
            let reader_handles: Vec<_> = (0..reader_count)
                .map(|_| {
                    s.spawn(|| {
                        let mut local_reporter = Reporter::new();
                        let mut rng = rand::rng();
                        barrier.wait();

                        for _ in 0..ops_per_reader {
                            let read_seq = read_seqno(seqno);
                            let idx: u64 = rng.random_range(0..config.num);
                            let key = make_sequential_key(idx, config.key_size);

                            let t = Instant::now();
                            let _ = tree.get(&key, read_seq);
                            local_reporter.record(t.elapsed().as_nanos() as u64);
                        }

                        local_reporter
                    })
                })
                .collect();

            // Writer thread — also borrows barrier by reference.
            let writer_handle = s.spawn(|| {
                barrier.wait();

                let writer_ops = ops_per_reader;
                for _ in 0..writer_ops {
                    let key = make_random_key(config.key_size);
                    let value = make_value(config.value_size);
                    let seq = seqno.fetch_add(1, Ordering::Relaxed);
                    tree.insert(key, value, seq);
                }
            });

            // Collect reader results.
            for handle in reader_handles {
                let local_reporter = handle.join().expect("reader thread panicked");
                reporter.merge(&local_reporter);
            }

            writer_handle.join().expect("writer thread panicked");
        });

        reporter.stop();

        Ok(())
    }
}
