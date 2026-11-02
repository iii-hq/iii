# Observe v2 benchmark baseline

This page summarizes the final recorded comparison of observe v2 against
0.25.0. The complete report and every raw run are preserved in the published
RC2 source snapshot:

- [Complete report, including earlier passes and SDK measurements](https://github.com/iii-hq/iii/blob/6cc82920f747acaebc1fa3ffdea3f87856054217/tech-specs/2026-10-29-observe-v2/bench-baseline.md).
- [Raw benchmark data (JSON)](https://github.com/iii-hq/iii/blob/6cc82920f747acaebc1fa3ffdea3f87856054217/tech-specs/2026-10-29-observe-v2/bench-baseline.json).

The raw data's SHA-256 is `638d9e6c301a982448317d99bb8054b3464db9c693e4efb16606f5a04130993a`.
These are historical measurements of the commits named below, not a benchmark
of every subsequent change to observe v2.

## Summary

The final pass ("Final pass (`ac89e90f6`)") measured the head of the PR at the time,
against 0.25. It counts a difference when every run of one version beats
every run of the other, and calls the two the same when their runs overlap.

- **Faster:** fan-out, by 9% to 60%; calls with observers, by 4% to 21%;
  calls with filtered observers, by 5% to 4.4x; creating and disposing of
  observers, by 13% to 3.4x.
- **Filters under load:** with 1,024 and 4,000 filtered observers, 0.25 drops
  about half the matching activations and 0.26 delivers all of them.
- **Less memory:** 23% to 52% less engine memory per observer.
- **About the same:** calls with no observers and routed calls with one
  (within 1.5%), and the `fanout` load test, which overloads both versions.
- **Slower, in every run:** adding and removing observers, by 1.3% to 3.3%
  (0.02 to 0.05 µs per observer), and the `filters` load test with 16
  observers, by 3.2%.
- **Not compared:** worker cleanup. When a worker disconnects, 0.26 holds its
  observers for a grace period and 0.25 deletes its triggers, so the two
  benches do different work.

## Final pass (`ac89e90f6`)

The final pass measured observe-v2 at `ac89e90f6` (2026-10-31), the head of
the PR at the time, against 0.25. It covers every engine bench row the earlier passes
compared for `worker_cleanup`, `control_plane_churn`, `observe_dispatch`,
`observe_fanout` and `observe_routed`, plus the `fanout`, `filters` and
`observers` load tests. Its runs are in the archived raw data under
`final_pass`.

| | |
|---|---|
| Machine | Apple M4 Max, 16 cores (12 performance, 4 efficiency), 64 GB, macOS 15.7.4 (24G517); load average 4.6 to 8.4 during the runs (including each bench's own threads) |
| 0.25.0 | `b0b86fd37`: the earlier passes' tree, its `observe_routed` port and its load-test binaries |
| 0.26.0 | `ac89e90f6` (observe-v2) |
| Rust | rustc 1.98.0, criterion 0.5.1; `cargo bench -p iii` in each tree (0.26 with `--locked`), each with its own target directory |
| Load test | Docker linux/arm64, rust:1.97 (rustc 1.97.1, Debian 13); VM with 16 vCPUs and 8 GB; container with CPUs 0-7 and 6 GB, engine on CPUs 4-7 and generator on 0-3; 5 s steps |

- **Engine benches.** Rows run with `--warm-up-time 1 --measurement-time 3`,
  versions interleaved bench by bench: `worker_cleanup` and
  `control_plane_churn` in 5 rounds, the others in 3. Figures are criterion's
  point estimate, as the median over the rounds.
- **Verdict.** Faster or slower (more or fewer, less or more memory) when
  every run of one version beats every run of the other, by any margin; the
  same when their runs overlap. There is no 5% threshold: a small difference
  that holds in every run counts. Chance alone puts every run of one version
  ahead about one time in ten with 3 runs per version, one in 126 with 5 and
  one in 6,435 with 8.
- **`worker_cleanup`.** Not compared: the two benches do different work.
  - When a worker disconnects, 0.26 holds its observers for the grace period
    and 0.25 deletes its triggers.
  - 0.26's observers row returns the engine, which holds the worker's suspended
    observers. Criterion drops it after the measurement.
  - 0.25's triggers row is changed the same way for this pass, which is not
    committed: it returns the registry that holds the worker's triggers.
  - The other registries are dropped inside the measurement in both versions.
  - 0.25 without that change measures 16.8, 46.5 and 166.3 µs.
- **Load test.** 8 runs per version for `fanout` and `filters`, and 3 for
  `observers`. Each round runs every version, and the order alternates round
  by round. Figures are medians.

### Engine benches

| Bench | 0.25 bench | 0.25 (µs) | 0.26 (µs) | Change | Verdict |
|---|---|---:|---:|---:|---|
| `worker_cleanup/observers/10` | `worker_cleanup/triggers/10` | 14.71 | 4.29 | -70.8% | not compared (different work) |
| `worker_cleanup/observers/50` | `worker_cleanup/triggers/50` | 44.40 | 7.41 | -83.3% | not compared (different work) |
| `worker_cleanup/observers/200` | `worker_cleanup/triggers/200` | 159.73 | 20.46 | -87.2% | not compared (different work) |
| `control_plane_churn/observers_add_remove/100` | `control_plane_churn/triggers_register_unregister/100` | 138.99 | 140.85 | +1.3% | slower than 0.25 |
| `control_plane_churn/observers_add_remove/1000` | `control_plane_churn/triggers_register_unregister/1000` | 1,420.30 | 1,446.60 | +1.9% | slower than 0.25 |
| `control_plane_churn/observers_add_remove/5000` | `control_plane_churn/triggers_register_unregister/5000` | 7,555.50 | 7,803.10 | +3.3% | slower than 0.25 |
| `observe_dispatch/default/0` | `observe_dispatch/auto/0` | 1.81 | 1.79 | -1.5% | faster than 0.25 |
| `observe_dispatch/default/1` | `observe_dispatch/auto/1` | 4.69 | 4.50 | -4.1% | faster than 0.25 |
| `observe_dispatch/default/8` | `observe_dispatch/auto/8` | 10.53 | 9.28 | -11.9% | faster than 0.25 |
| `observe_dispatch/default/64` | `observe_dispatch/auto/64` | 76.52 | 63.98 | -16.4% | faster than 0.25 |
| `observe_dispatch/filtered/16` | `observe_dispatch/filtered/16` | 4.75 | 4.50 | -5.2% | faster than 0.25 |
| `observe_dispatch/filtered/256` | `observe_dispatch/filtered/256` | 5.02 | 4.52 | -10.0% | faster than 0.25 |
| `observe_dispatch/filtered/1024` | `observe_dispatch/filtered/1024` | 19.92 | 4.51 | -77.4% | faster than 0.25 |
| `observe_dispatch/filtered/4000` | none | | 4.50 | | 0.25 has no such row |
| `observe_fanout/1` | `trigger_fanout/1` | 11.34 | 10.37 | -8.5% | faster than 0.25 |
| `observe_fanout/8` | `trigger_fanout/8` | 28.82 | 23.07 | -20.0% | faster than 0.25 |
| `observe_fanout/32` | `trigger_fanout/32` | 85.01 | 50.24 | -40.9% | faster than 0.25 |
| `observe_fanout/128` | `trigger_fanout/128` | 322.10 | 129.24 | -59.9% | faster than 0.25 |
| `observe_routed/observed/0` | `observe_routed/observed/0` | 33.97 | 34.14 | +0.5% | the same as 0.25 |
| `observe_routed/observed/1` | `observe_routed/observed/1` | 35.79 | 35.46 | -0.9% | faster than 0.25 |
| `observe_routed/observed/8` | `observe_routed/observed/8` | 43.45 | 39.63 | -8.8% | faster than 0.25 |
| `observe_routed/observed/64` | `observe_routed/observed/64` | 91.23 | 72.41 | -20.6% | faster than 0.25 |

Of the 18 rows compared, 14 are faster than 0.25, 3 are slower and 1 is the
same. The slower ones are `control_plane_churn/observers_add_remove`: 1.3%,
1.9% and 3.3% slower with 100, 1,000 and 5,000 observers, in all 5 runs. That
is 1.41, 1.45 and 1.56 µs per add and remove, against 1.39, 1.42 and 1.51 µs
per trigger.

### Load test

#### `fanout`

| Observers a function | Calls/s, 0.25 | Calls/s, 0.26 | Change | Activations/s, 0.25 | Activations/s, 0.26 | Change | Delivered, 0.25 / 0.26 |
|---:|---:|---:|---:|---:|---:|---:|---|
| 0 | 178.6k | 183.5k | +2.7% | | | | |
| 1 | 98.0k | 96.5k | -1.5% | 71.7k | 68.0k | -5.2% | 74% / 71% |
| 4 | 57.0k | 62.9k | +10.4% | 193.2k | 192.6k | -0.3% | 83% / 77% |
| 16 | 71.1k | 73.5k | +3.4% | 203.2k | 228.1k | +12.3% | 18% / 19% |
| 64 | 68.4k | 70.6k | +3.2% | 193.8k | 199.9k | +3.1% | 5% / 5% |

#### `filters`

| Filtered observers | Calls/s, 0.25 | Calls/s, 0.26 | Change | Activations/s, 0.25 | Activations/s, 0.26 | Delivered, 0.25 / 0.26 |
|---:|---:|---:|---:|---:|---:|---|
| 16 | 101.8k | 98.5k | -3.2% | 101.8k | 98.5k | 100% / 100% |
| 256 | 95.9k | 97.8k | +2.0% | 95.9k | 97.8k | 100% / 100% |
| 1,024 | 82.6k | 97.7k | +18.2% | 34.6k | 97.7k | 57% / 100% |
| 4,000 | 33.7k | 87.0k | +158.0% | 5.0k | 87.0k | 49% / 100% |

#### `observers`

| Step | 0.25 | 0.26 | Change |
|---|---:|---:|---:|
| Create up to 1,000 (per second) | 27.0k | 50.4k | +86% |
| Create up to 4,000 (per second) | 41.1k | 74.6k | +82% |
| Create up to 16,000 (per second) | 4.8k | 5.4k | +13% |
| Create up to 64,000 (per second) | 4.8k | 16.3k | +239% |
| Dispose 64,000 (per second) | 4.6k | 10.3k | +126% |
| Engine memory per observer at rest, 1,000 (KiB) | 9.6 | 4.6 | -52% |
| Engine memory per observer at rest, 4,000 (KiB) | 7.6 | 5.2 | -32% |
| Engine memory per observer at rest, 16,000 (KiB) | 8.0 | 6.2 | -23% |
| Engine memory per observer at rest, 64,000 (KiB) | 7.9 | 5.4 | -32% |

- **`fanout`:**
  - Both versions are overloaded. At every step with observers they drop
    activations as `OVERFLOW`: 17% to 29% of them with 1 and 4 observers, and
    81% to 95% with 16 and 64. The test shows how much each version gets
    through when overloaded.
  - With 0 observers, 0.26 makes 2.7% more calls per second, in every run.
  - At every step with observers, the two versions' runs overlap in calls and
    in activations delivered per second, so the two are the same there. The
    medians differ by −5.2% to +12.3%.
  - With 1 observer, 0.26 delivers 71% of the activations against 74%, lower
    in every run.
- **`filters`:** each call matches one observer.
  - 0.26 makes 18% more calls per second with 1,024 observers and 2.6x the
    calls with 4,000, and delivers every activation. 0.25 delivers 57% and 49%
    of them and drops the rest as `OVERFLOW`.
  - 0.26 makes 2.0% more calls per second with 256 observers and 3.2% fewer
    with 16, each in every run.
- **`observers`:**
  - 0.26 creates observers faster at every step: 1.9x up to 1,000, 1.8x up
    to 4,000, 13% faster up to 16,000 and 3.4x up to 64,000. It disposes of
    64,000 2.3x as fast.
  - The step from 4,000 to 16,000 also opens twelve worker connections. It
    takes about 2.5 s in 0.25 and 2.2 s in 0.26.
  - 0.26 holds 23% to 52% less engine memory per observer at rest, in every
    run.

## Reproducing

```sh
# Engine benches (each version in its own checkout and target directory)
cargo bench -p iii --bench control_plane_churn_bench -- --save-baseline r1 --noplot
cargo bench -p iii --bench worker_cleanup_bench -- --save-baseline r1 --noplot
cargo bench -p iii --bench observe_dispatch_bench -- --save-baseline r1 --noplot
cargo bench -p iii --bench observe_fanout_bench -- --save-baseline r1 --noplot   # 0.25: trigger_fanout_bench
cargo bench -p iii --bench observe_routed_bench -- --save-baseline r1 --noplot

# Rust SDK (criterion rows, then the paced pass at 5,000 calls/s)
cargo bench -p iii-sdk --bench observe_bench -- --save-baseline r1 --noplot

# The filter index against a scan (decision 55)
cargo bench -p iii-filter --bench filter_bench -- eq_1_of --noplot

# Node SDK
pnpm --filter iii-sdk bench

# Go SDK
(cd sdk/packages/go/iii && go test -run '^$' -bench '^BenchmarkIsObserved$' -benchmem -count 10 .)
(cd sdk/packages/go/iii && go test -run '^$' -bench '^BenchmarkLocalCall$' -benchmem -count 10 .)   # decision 55

# Load test (Linux; here in Docker, linux/arm64)
cargo build --release -p iii -p iii-loadtest
target/release/iii-loadtest suite --profiles host --engine-cpus 4-7 --load-cpus 0-3 \
  --scenarios rpc,fanout,filters,observers,source,match-keys --port 58010
```
