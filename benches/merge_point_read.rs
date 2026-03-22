use criterion::{criterion_group, criterion_main, Criterion};
use lsm_tree::{AbstractTree, Cache, Config, MergeOperator, SequenceNumberCounter, UserValue};
use std::sync::Arc;
use tempfile::tempdir;

/// Simple counter merge operator for benchmarks.
struct CounterMerge;

impl MergeOperator for CounterMerge {
    fn merge(
        &self,
        _key: &[u8],
        base_value: Option<&[u8]>,
        operands: &[&[u8]],
    ) -> lsm_tree::Result<UserValue> {
        let mut counter: i64 = match base_value {
            Some(bytes) if bytes.len() == 8 => {
                i64::from_le_bytes(bytes.try_into().expect("checked"))
            }
            _ => 0,
        };
        for op in operands {
            if op.len() == 8 {
                counter += i64::from_le_bytes((*op).try_into().expect("checked"));
            }
        }
        Ok(counter.to_le_bytes().to_vec().into())
    }
}

fn merge_point_read_deep_tree(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge point read");
    group.sample_size(100);

    for table_count in [10, 50, 100] {
        // --- Uncached: cold disk reads ---
        let folder = tempdir().unwrap();
        let tree = Config::new(
            &folder,
            SequenceNumberCounter::default(),
            SequenceNumberCounter::default(),
        )
        .use_cache(Arc::new(Cache::with_capacity_bytes(0)))
        .with_merge_operator(Some(Arc::new(CounterMerge)))
        .open()
        .unwrap();

        let mut seqno = 0u64;

        // Base value on disk
        tree.insert("counter", 100_i64.to_le_bytes(), seqno);
        seqno += 1;
        tree.flush_active_memtable(0).unwrap();

        // Create many tables with unrelated keys (bloom should reject these)
        for i in 1..table_count {
            let key = format!("other_{i:04}");
            tree.insert(key, 0_i64.to_le_bytes(), seqno);
            seqno += 1;
            tree.flush_active_memtable(0).unwrap();
        }

        // Merge operand in active memtable
        tree.merge("counter", 1_i64.to_le_bytes(), seqno);
        seqno += 1;

        group.bench_function(format!("merge get, {table_count} tables (uncached)"), |b| {
            b.iter(|| {
                let val = tree.get("counter", seqno).unwrap().unwrap();
                let n = i64::from_le_bytes((*val).try_into().unwrap());
                assert_eq!(n, 101);
            });
        });

        // --- Cached: warm block cache ---
        let folder2 = tempdir().unwrap();
        let tree_cached = Config::new(
            &folder2,
            SequenceNumberCounter::default(),
            SequenceNumberCounter::default(),
        )
        .use_cache(Arc::new(Cache::with_capacity_bytes(64 * 1_024 * 1_024)))
        .with_merge_operator(Some(Arc::new(CounterMerge)))
        .open()
        .unwrap();

        let mut s = 0u64;
        tree_cached.insert("counter", 100_i64.to_le_bytes(), s);
        s += 1;
        tree_cached.flush_active_memtable(0).unwrap();

        for i in 1..table_count {
            let key = format!("other_{i:04}");
            tree_cached.insert(key, 0_i64.to_le_bytes(), s);
            s += 1;
            tree_cached.flush_active_memtable(0).unwrap();
        }

        tree_cached.merge("counter", 1_i64.to_le_bytes(), s);
        s += 1;

        // Warm the cache
        let _ = tree_cached.get("counter", s).unwrap();

        group.bench_function(format!("merge get, {table_count} tables (cached)"), |b| {
            b.iter(|| {
                let val = tree_cached.get("counter", s).unwrap().unwrap();
                let n = i64::from_le_bytes((*val).try_into().unwrap());
                assert_eq!(n, 101);
            });
        });
    }
}

criterion_group!(benches, merge_point_read_deep_tree);
criterion_main!(benches);
