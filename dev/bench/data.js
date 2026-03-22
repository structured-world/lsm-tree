window.BENCHMARK_DATA = {
  "lastUpdate": 1774198823042,
  "repoUrl": "https://github.com/structured-world/lsm-tree",
  "entries": {
    "lsm-tree db_bench": [
      {
        "commit": {
          "author": {
            "email": "mail@polaz.com",
            "name": "Dmitry Prudnikov",
            "username": "polaz"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "b9c68970f2f45ee3c1df1d2a6bf8d17d21616a5d",
          "message": "feat(testing): db_bench suite + property-based model tests (#45)\n\n## Summary\n\n- Add `tools/db_bench/` standalone crate with 9 RocksDB\ndb_bench-compatible benchmark workloads\n- Add proptest-based property tests with BTreeMap MVCC oracle\n- Property tests found 2 MVCC bugs — both fixed in PR #56 (issues #52,\n#53)\n\n## db_bench Workloads\n\n| Benchmark | Description | M1 Mac result |\n|-----------|-------------|---------------|\n| `fillseq` | Sequential inserts | 2,738K ops/s |\n| `fillrandom` | Random inserts | 514K ops/s |\n| `readrandom` | Random point reads | 375K ops/s |\n| `readseq` | Full forward scan | 467 MB/s |\n| `seekrandom` | Random seek + next | 270K ops/s |\n| `prefixscan` | Prefix scans | 244K ops/s |\n| `overwrite` | Random overwrites | 299K ops/s |\n| `mergerandom` | Hot key compaction stress | 74K ops/s |\n| `readwhilewriting` | Concurrent read+write (4T) | 665K ops/s |\n\nRun: `cd tools/db_bench && cargo run --release -- --benchmark fillseq\n--num 1000000`\n\n## Property Tests\n\n- `prop_btreemap_oracle.rs` — Insert/Remove/Flush/Compact vs BTreeMap\noracle\n- `prop_range_tombstone.rs` — Range tombstone focused\n- `prop_mvcc.rs` — Snapshot isolation at historical seqnos\n- `prop_regression_rt_tombstone.rs` — 7 regression tests (all passing)\n\n## Bugs Found & Fixed\n\n1. **L0 stale reads** (#52): 3+ L0 SSTs + non-empty active memtable →\npoint reads return stale values — **fixed in PR #56**\n2. **RT + tombstone** (#53): Point tombstone invisible when range\ntombstone exists in prior SST — **fixed in PR #56**\n\nAll regression tests and proptests now run without `#[ignore]`.\n\n## Test Plan\n\n- [x] `cargo test --all-features` — all suites pass, 0 failures\n- [x] `cargo clippy --all-features -- -D warnings` — clean\n- [x] All 9 db_bench workloads produce correct output\n- [x] JSON output mode works (`--json`)\n- [x] CI: `PROPTEST_CASES=32` for bounded CI runtime\n\nCloses #42 (partial: db_bench + property tests)\n\n<!-- This is an auto-generated comment: release notes by coderabbit.ai\n-->\n## Summary by CodeRabbit\n\n* **New Features**\n* Added `db_bench` benchmarking tool with multiple workload types\n(sequential fill, random fill, read operations, merge operations, and\nrange scans).\n\n* **Tests**\n* Added property-based tests for MVCC snapshot consistency, range\ntombstone behavior validation, and oracle-based verification.\n\n* **Chores**\n* Enhanced test infrastructure with improved timeout configuration for\nproperty-based tests.\n* Updated CI/CD pipeline with automated benchmark execution and GitHub\nPages reporting.\n<!-- end of auto-generated comment: release notes by coderabbit.ai -->",
          "timestamp": "2026-03-22T15:01:23+02:00",
          "tree_id": "013e544b13688410ea97f35f9f3751378a99f845",
          "url": "https://github.com/structured-world/lsm-tree/commit/b9c68970f2f45ee3c1df1d2a6bf8d17d21616a5d"
        },
        "date": 1774184552180,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "fillseq",
            "value": 1863915.4752355856,
            "unit": "ops/sec",
            "extra": "P50: 0.4us | P99: 2.3us | P99.9: 5.3us\nthreads: 1 | elapsed: 0.11s | num: 200000"
          },
          {
            "name": "fillrandom",
            "value": 1078840.3879110864,
            "unit": "ops/sec",
            "extra": "P50: 0.8us | P99: 1.6us | P99.9: 3.8us\nthreads: 1 | elapsed: 0.19s | num: 200000"
          },
          {
            "name": "readrandom",
            "value": 655440.3305655628,
            "unit": "ops/sec",
            "extra": "P50: 1.3us | P99: 5.4us | P99.9: 11.2us\nthreads: 1 | elapsed: 0.31s | num: 200000"
          },
          {
            "name": "readseq",
            "value": 2412298.3602015926,
            "unit": "ops/sec",
            "extra": "P50: 0.2us | P99: 4.1us | P99.9: 8.0us\nthreads: 1 | elapsed: 0.08s | num: 200000"
          },
          {
            "name": "seekrandom",
            "value": 394519.8797906606,
            "unit": "ops/sec",
            "extra": "P50: 2.2us | P99: 6.3us | P99.9: 12.4us\nthreads: 1 | elapsed: 0.51s | num: 200000"
          },
          {
            "name": "prefixscan",
            "value": 191850.88557212878,
            "unit": "ops/sec",
            "extra": "P50: 4.8us | P99: 7.5us | P99.9: 16.1us\nthreads: 1 | elapsed: 1.04s | num: 200000"
          },
          {
            "name": "overwrite",
            "value": 929551.8754039077,
            "unit": "ops/sec",
            "extra": "P50: 0.9us | P99: 3.1us | P99.9: 9.9us\nthreads: 1 | elapsed: 0.22s | num: 200000"
          },
          {
            "name": "mergerandom",
            "value": 697430.4566199599,
            "unit": "ops/sec",
            "extra": "P50: 0.4us | P99: 0.6us | P99.9: 0.9us\nthreads: 1 | elapsed: 0.29s | num: 200000"
          },
          {
            "name": "readwhilewriting",
            "value": 523817.1499636354,
            "unit": "ops/sec",
            "extra": "P50: 1.6us | P99: 7.9us | P99.9: 13.7us\nthreads: 1 | elapsed: 0.38s | num: 200000"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "mail@polaz.com",
            "name": "Dmitry Prudnikov",
            "username": "polaz"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "9c4b06595c7b13dcb1a584792cf1f810769cbc16",
          "message": "refactor: unify merge resolution via bloom-filtered iterator pipeline (#69)\n\n## Summary\n\n- Replace hand-rolled `resolve_merge_get()` with\n`resolve_merge_via_pipeline()` that reuses `Merger → MvccStream` on a\n`key..=key` range\n- Add standard bloom pre-filtering\n(`Table::bloom_may_contain_key_hash()`) to skip many disk tables for\npoint reads\n- Eliminate duplicated operand collection / RT suppression / Indirection\nlogic between point reads and range scans\n\nNet **-143 lines** — merge resolution now lives in one place\n(`MvccStream`).\n\n## Changes\n\n| File | What |\n|------|------|\n| `table/mod.rs` | Extract `bloom_may_contain_hash()` base, add\n`bloom_may_contain_key_hash()` |\n| `range.rs` | Add `key_hash` to `IterState`, `bloom_passes()` helper\nfor unified prefix+key bloom |\n| `tree/mod.rs` | `resolve_merge_via_pipeline()` replaces ~150-line\n`resolve_merge_get()` |\n| `memtable/mod.rs` | Remove unused `get_all_for_key()` and its tests |\n| `tests/merge_operator.rs` | Update comments referencing old function\nname |\n\n## Test plan\n\n- [x] `cargo check` — 0 warnings, 0 errors\n- [x] `cargo test` — 757 passed, 0 failed\n- [x] All 44 merge operator tests pass unchanged\n- [ ] Benchmark point-read latency on 100-table tree within 5% of\nbaseline\n\nCloses #46\n\n<!-- This is an auto-generated comment: release notes by coderabbit.ai\n-->\n## Summary by CodeRabbit\n\n* **Improvements**\n* Enhanced bloom-filter pre-filtering for single- and multi-table scans\nwith optional key-hash checks and consolidated pass/fail logic;\nprefix-based skip metrics adjusted.\n\n* **Refactor**\n* Merge resolution unified into a pipeline-based point-read path;\nobsolete per-key retrieval pathway removed.\n\n* **Tests**\n* Added and updated tests validating prefix/bloom behavior and merge\nresolution with overlapping/non-matching tables.\n\n* **Chores**\n  * Added a benchmark for merge point-read performance.\n<!-- end of auto-generated comment: release notes by coderabbit.ai -->",
          "timestamp": "2026-03-22T17:37:29+02:00",
          "tree_id": "96ae1889c1cb52cea1404ed15987ea348fbe6967",
          "url": "https://github.com/structured-world/lsm-tree/commit/9c4b06595c7b13dcb1a584792cf1f810769cbc16"
        },
        "date": 1774193921253,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "fillseq",
            "value": 1961848.116249624,
            "unit": "ops/sec",
            "extra": "P50: 0.4us | P99: 1.8us | P99.9: 3.8us\nthreads: 1 | elapsed: 0.10s | num: 200000"
          },
          {
            "name": "fillrandom",
            "value": 792196.3745489408,
            "unit": "ops/sec",
            "extra": "P50: 1.1us | P99: 2.4us | P99.9: 9.5us\nthreads: 1 | elapsed: 0.25s | num: 200000"
          },
          {
            "name": "readrandom",
            "value": 529814.6491663025,
            "unit": "ops/sec",
            "extra": "P50: 1.7us | P99: 4.5us | P99.9: 10.3us\nthreads: 1 | elapsed: 0.38s | num: 200000"
          },
          {
            "name": "readseq",
            "value": 3118700.8577534496,
            "unit": "ops/sec",
            "extra": "P50: 0.2us | P99: 3.3us | P99.9: 5.9us\nthreads: 1 | elapsed: 0.06s | num: 200000"
          },
          {
            "name": "seekrandom",
            "value": 358026.9349713834,
            "unit": "ops/sec",
            "extra": "P50: 2.4us | P99: 5.6us | P99.9: 10.6us\nthreads: 1 | elapsed: 0.56s | num: 200000"
          },
          {
            "name": "prefixscan",
            "value": 232849.66026990075,
            "unit": "ops/sec",
            "extra": "P50: 4.0us | P99: 5.1us | P99.9: 11.4us\nthreads: 1 | elapsed: 0.86s | num: 200000"
          },
          {
            "name": "overwrite",
            "value": 729768.2800349746,
            "unit": "ops/sec",
            "extra": "P50: 1.2us | P99: 3.3us | P99.9: 9.1us\nthreads: 1 | elapsed: 0.27s | num: 200000"
          },
          {
            "name": "mergerandom",
            "value": 735288.3921293583,
            "unit": "ops/sec",
            "extra": "P50: 0.4us | P99: 0.9us | P99.9: 1.3us\nthreads: 1 | elapsed: 0.27s | num: 200000"
          },
          {
            "name": "readwhilewriting",
            "value": 433143.6017810614,
            "unit": "ops/sec",
            "extra": "P50: 2.1us | P99: 6.3us | P99.9: 12.8us\nthreads: 1 | elapsed: 0.46s | num: 200000"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "mail@polaz.com",
            "name": "Dmitry Prudnikov",
            "username": "polaz"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "9f3ee1eb92efa6bd5cb14068147b3a4c35f1c2cd",
          "message": "fix(testing): prevent proptest oracle timeouts in CI (#95)\n\n## Summary\n\n- Increase nextest slow-timeout for prop tests from 120s to 240s\n- Set `PROPTEST_CASES=32` in `codecov` and `cross` CI jobs (were\ndefaulting to 256)\n- Reduce op sequence ranges: btreemap `200→100`, range_tombstone\n`300→150`\n- Add `fork: false` to all proptest configs to skip subprocess overhead\n\n## Root Cause\n\nThree prop tests (`prop_btreemap_oracle`, `prop_mvcc`,\n`prop_range_tombstone`) were hitting the 120s nextest terminate\nthreshold. Contributing factors:\n1. `codecov` and `cross` jobs didn't set `PROPTEST_CASES` — ran 256\ncases instead of 32\n2. Large op sequence ranges (up to 300 ops/case) with expensive\nflush+compact I/O\n3. Tight nextest budget (`30s × 4 = 120s`) left no headroom for slower\nCI runners\n\n## Test Plan\n\n- [x] All prop tests pass locally with `PROPTEST_CASES=32` (13s + 8s +\n29s)\n- [x] Full test suite passes (`cargo test --all-features`)\n- [x] `cargo clippy --all-features -- -D warnings` clean\n- [x] `cargo fmt --check` clean\n\nCloses #93",
          "timestamp": "2026-03-22T18:56:52+02:00",
          "tree_id": "f84a1baf516c88b0da3926cbb29a3f5d227a2ee1",
          "url": "https://github.com/structured-world/lsm-tree/commit/9f3ee1eb92efa6bd5cb14068147b3a4c35f1c2cd"
        },
        "date": 1774198822101,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "fillseq",
            "value": 1850147.1981736235,
            "unit": "ops/sec",
            "extra": "P50: 0.4us | P99: 2.3us | P99.9: 5.3us\nthreads: 1 | elapsed: 0.11s | num: 200000"
          },
          {
            "name": "fillrandom",
            "value": 1047616.0838632599,
            "unit": "ops/sec",
            "extra": "P50: 0.8us | P99: 1.7us | P99.9: 3.9us\nthreads: 1 | elapsed: 0.19s | num: 200000"
          },
          {
            "name": "readrandom",
            "value": 645505.836350702,
            "unit": "ops/sec",
            "extra": "P50: 1.3us | P99: 5.3us | P99.9: 11.5us\nthreads: 1 | elapsed: 0.31s | num: 200000"
          },
          {
            "name": "readseq",
            "value": 2378416.168197215,
            "unit": "ops/sec",
            "extra": "P50: 0.3us | P99: 4.3us | P99.9: 8.8us\nthreads: 1 | elapsed: 0.08s | num: 200000"
          },
          {
            "name": "seekrandom",
            "value": 373530.3660194361,
            "unit": "ops/sec",
            "extra": "P50: 2.3us | P99: 6.4us | P99.9: 12.9us\nthreads: 1 | elapsed: 0.54s | num: 200000"
          },
          {
            "name": "prefixscan",
            "value": 195786.71109249876,
            "unit": "ops/sec",
            "extra": "P50: 4.8us | P99: 6.9us | P99.9: 15.0us\nthreads: 1 | elapsed: 1.02s | num: 200000"
          },
          {
            "name": "overwrite",
            "value": 869153.7675556025,
            "unit": "ops/sec",
            "extra": "P50: 0.9us | P99: 3.2us | P99.9: 10.8us\nthreads: 1 | elapsed: 0.23s | num: 200000"
          },
          {
            "name": "mergerandom",
            "value": 616324.3716153931,
            "unit": "ops/sec",
            "extra": "P50: 0.4us | P99: 0.6us | P99.9: 0.9us\nthreads: 1 | elapsed: 0.32s | num: 200000"
          },
          {
            "name": "readwhilewriting",
            "value": 484385.11089586595,
            "unit": "ops/sec",
            "extra": "P50: 1.7us | P99: 10.3us | P99.9: 17.5us\nthreads: 1 | elapsed: 0.41s | num: 200000"
          }
        ]
      }
    ]
  }
}