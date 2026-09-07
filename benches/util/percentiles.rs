// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026-present, Dmitry Prudnikov

//! Tail-latency reporting shared by the flush benches.
//!
//! Included with `#[path]` rather than published as a bench target: a file
//! directly under `benches/` would be auto-discovered as a benchmark of its
//! own. Lives here so the flush benches do not each carry a copy.

use std::time::Duration;

/// Reports per-operation tail latency (P50/P95/P99) to stderr.
///
/// Criterion's summary surfaces only mean and confidence interval. In these
/// benches one iteration is one whole operation, so the per-iteration
/// durations collected by the caller are the per-operation latencies, and
/// their tail is what a regression shows up in first.
pub fn report_percentiles(label: &str, mut samples: Vec<Duration>) {
    if samples.is_empty() {
        return;
    }
    samples.sort_unstable();
    let pick = |p: f64| {
        let idx = (((samples.len() - 1) as f64) * p).round() as usize;
        samples[idx.min(samples.len() - 1)]
    };
    eprintln!(
        "  [{label}] n={} P50={:?} P95={:?} P99={:?}",
        samples.len(),
        pick(0.50),
        pick(0.95),
        pick(0.99),
    );
}
