// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026-present, Dmitry Prudnikov

//! Attributes the small-flush CPU cost to the parts that make it up.
//!
//! Same shape as `benches/flush.rs`, memfs only (so no syscall or fsync is in
//! the number), 1000 rows, but each arm switches off one more piece of the
//! write path. Read the arms as differences:
//!
//! | difference | what it costs |
//! |---|---|
//! | `zstd1_1000` − `none_1000` | the block codec |
//! | `none_1000` − `none_nofilter_1000` | the membership filter build |
//! | `none_nofilter_1000` − `none_bare_1000` | the block-locator ribbon build |
//! | `none_bare_1000` | block encode, index, memtable rotation, manifest edit |
//!
//! The 1000-row size is deliberate: at that size the flush is dominated by its
//! fixed cost, which is what the overwrite head-to-head against RocksDB pays on
//! every iteration.

#![expect(
    clippy::expect_used,
    reason = "benchmark setup favors concise panic messages"
)]

use criterion::{Criterion, criterion_group, criterion_main};
use lsm_tree::config::{CompressionPolicy, FilterPolicy, LocatorPolicy};
use lsm_tree::{AbstractTree, AnyTree, CompressionType, Config, SequenceNumberCounter};
use std::time::{Duration, Instant};

fn build_unflushed_tree(
    keys: u64,
    compression: CompressionType,
    filters: bool,
    locator: bool,
) -> AnyTree {
    let config = Config::new(
        "/bench",
        SequenceNumberCounter::default(),
        SequenceNumberCounter::default(),
    )
    .with_fs(lsm_tree::fs::MemFs::new());

    let config = if filters {
        config
    } else {
        config.filter_policy(FilterPolicy::disabled())
    };
    let config = if locator {
        config
    } else {
        config.locator_policy(LocatorPolicy::disabled())
    };

    let tree = config
        .data_block_compression_policy(CompressionPolicy::all(compression))
        .open()
        .expect("open");

    for i in 0..keys {
        let key = format!("key_{i:08}");
        let value = format!("row-{i}-{}", "the quick brown fox ".repeat(8));
        tree.insert(key, value, i);
    }
    tree
}

fn bench_split(c: &mut Criterion) {
    let mut group = c.benchmark_group("flush_split");
    group.sample_size(10);

    let zstd1 = CompressionType::zstd(1).expect("valid level");
    for (label, compression, filters, locator) in [
        ("zstd1_1000", zstd1, true, true),
        ("none_1000", CompressionType::None, true, true),
        ("none_nofilter_1000", CompressionType::None, false, true),
        ("none_bare_1000", CompressionType::None, false, false),
    ] {
        group.bench_function(label, |b| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let tree = build_unflushed_tree(1_000, compression, filters, locator);
                    let start = Instant::now();
                    tree.flush_active_memtable(0).expect("flush");
                    total += start.elapsed();
                    std::hint::black_box(&tree);
                }
                total
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_split);
criterion_main!(benches);
