pub mod fillrandom;
pub mod fillseq;
pub mod mergerandom;
pub mod overwrite;
pub mod prefixscan;
pub mod readrandom;
pub mod readseq;
pub mod readwhilewriting;
pub mod seekrandom;

use crate::config::BenchConfig;
use crate::reporter::Reporter;
use lsm_tree::AnyTree;
use std::sync::atomic::AtomicU64;

/// All benchmark workloads implement this trait.
pub trait Workload {
    /// Human-readable name of the benchmark.
    fn name(&self) -> &'static str;

    /// Run the benchmark, recording latencies into the reporter.
    fn run(
        &self,
        tree: &AnyTree,
        config: &BenchConfig,
        seqno: &AtomicU64,
        reporter: &mut Reporter,
    ) -> lsm_tree::Result<()>;
}

/// Create a workload by name.
pub fn create_workload(name: &str) -> Option<Box<dyn Workload>> {
    match name {
        "fillseq" => Some(Box::new(fillseq::FillSeq)),
        "fillrandom" => Some(Box::new(fillrandom::FillRandom)),
        "readrandom" => Some(Box::new(readrandom::ReadRandom)),
        "readseq" => Some(Box::new(readseq::ReadSeq)),
        "seekrandom" => Some(Box::new(seekrandom::SeekRandom)),
        "prefixscan" => Some(Box::new(prefixscan::PrefixScan)),
        "overwrite" => Some(Box::new(overwrite::Overwrite)),
        "mergerandom" => Some(Box::new(mergerandom::MergeRandom)),
        "readwhilewriting" => Some(Box::new(readwhilewriting::ReadWhileWriting)),
        _ => None,
    }
}

/// List all available benchmark names.
pub fn available_benchmarks() -> &'static [&'static str] {
    &[
        "fillseq",
        "fillrandom",
        "readrandom",
        "readseq",
        "seekrandom",
        "prefixscan",
        "overwrite",
        "mergerandom",
        "readwhilewriting",
    ]
}
