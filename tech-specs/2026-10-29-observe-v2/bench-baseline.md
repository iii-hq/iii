# Observe v2 benchmark baseline

What observe v2 (0.26.0) costs, measured against the performance budget of the
spec (§13) and against 0.25.0 on the same machine. Every number here is in
`bench-baseline.json`, run by run, with the machine, the toolchain and the
commits.

## Summary

The §13 budget, `isObserved()` and Node and Go figures are of observe-v2 at
`e0d87164c`. The observer churn fix (`d37f78f42`) was measured again on the
engine's observe, control-plane and cleanup benches and four load tests
("Observer churn fix").

- Every row of the §13 budget is within budget on the Rust host, in a tight
  loop and at 5,000 calls/s. At that rate the p99 of a local call is 6.7 to
  10.5 µs whatever its observers, so the 5 µs of the 10-observer row holds for
  its mean and p90, not its p99.
- `isObserved()` takes 0.436 ns in Rust, 1.79 ns in Go and 28.9 ns in Node
  (the Node bench's floor: an empty function measures 29.3 ns). Rust and Go
  measure no allocation.
- An unobserved routed call takes one `invokefunction` and one
  `invocationresult` and costs what it cost in 0.25 (34.9 µs against 35.1 µs).
  An observed one takes the same two frames, whatever the number of observers.
- Engine benches against 0.25, with the observer churn fix (`d37f78f42`): 64
  rows the same within noise, 20 faster and 10 slower beyond noise. The 33
  rows of `control_plane_churn`, `worker_cleanup`, `observe_dispatch`,
  `observe_fanout` and `observe_routed` come from the fix's pass; the other
  61 rows (51 the same, 10 faster) were measured on observe-v2 before the fix
  only. Four of the slower rows are small and the same as before the fix
  (`observe_fanout/1` +15%, `observe_fanout/8` +6%, `observe_routed/observed/8`
  and `/64` +7% and +8%). Six are larger: adding and removing an observer of
  one function (`control_plane_churn/observers_add_remove`) costs 4.0, 4.3 and
  4.9 µs with 100, 1,000 and 5,000 observers, 2.9x to 3.3x what a trigger cost
  in 0.25 (6.3, 21.5 and 91 µs before the fix); a worker's cleanup
  (`worker_cleanup/observers`) costs 2.7x to 4.0x per observer what it cost
  per trigger.
- Load test against 0.25. On observe-v2 before the fix, `rpc` is the same and
  `source` delivers 200k activations/s sustained where 0.25's `emit`
  delivered 100k. With the fix, `filters` takes 5% to 84% fewer calls per
  second (16 to 4,000 observers) and drops nothing where 0.25 dropped most of
  its observations; `fanout` takes 14% to 31% fewer calls per second from 4
  observers a function on and delivers the same activations within noise; the
  `observers` scenario creates 64,000 observers 3.4 times as fast as 0.25 and
  disposes of them 2.3 times as fast, and holds 12% to 24% more engine memory
  per observer at rest.
- Decision 55 ("Filters evaluated by the engine"): the engine matches a call
  of a function with 1,024 or 4,000 observers, each filtering on its own
  value of `input.k`, in 5.61 and 6.04 µs (`observe_dispatch/filtered`),
  against 21.8 and 74.8 µs for observe-v2's scan, and a routed call of such a
  function costs what an unobserved one costs plus one activation, whatever
  the number of observers (`observe_routed/filtered`, 36.5 µs with 4,000).
  The rows of unfiltered observers run observe-v2's code.

## Setup

| | |
|---|---|
| Machine | Apple M4 Max, 16 cores (12 performance, 4 efficiency), 64 GB, macOS 15.7.4 (24G517) |
| Other load | one unrelated process at 100% of one core and a browser renderer at about 70% of one throughout; load average 6 to 10. Versions alternate run by run, and every run is repeated. |
| 0.25.0 | `b0b86fd37` (main; observe-v2 branched from it) |
| 0.26.0 | `e0d87164c` (observe-v2), with the bench output this file's commit adds: `observe_routed_bench` prints the frames per call, `observe_bench` runs each local call again at 5,000 calls/s. Neither changes the code measured. |
| 0.26.0 with the observer churn fix | `d37f78f42` (`ov2/observer-churn`, on observe-v2 `b05994f93`), measured in "Observer churn fix" |
| Rust | rustc 1.98.0 (88d9e12ae 2026-08-18), cargo 1.98.0, aarch64-apple-darwin; criterion 0.5.1; the `bench` profile is the release profile (lto, codegen-units 1, opt-level 2). The load test builds with rustc 1.97.1 in its image. |
| Node | Node 22.13.0, vitest 4.1.12, pnpm 10.19.0 |
| Go | go1.25.14 linux/arm64 (golang:1.25 image, Docker, 8 CPUs) |
| Load test | `iii-loadtest`, `host` profile, each version built and run in the rust:1.97 image (Debian 13, linux/arm64, rustc 1.97.1) under Docker Desktop 29.2.0, VM with 16 vCPUs and 8 GB, kernel 6.12.67-linuxkit. The container has CPUs 0-7 and 6 GB; the engine runs on CPUs 4-7 and the load generator on 0-3. Steps of 10 s after 2 s of warm-up; observability and log level as shipped. |

## Method

- **Engine benches.** `cargo bench -p iii --bench <name> -- --save-baseline rN
  --noplot` in each tree, each with its own target directory. Every bench that
  both versions have runs bench by bench, 0.25 then 0.26, in two rounds;
  `observe_dispatch`, `observe_fanout` (with 0.25's `trigger_fanout`),
  `observe_routed` and `invoke_function_payload_sizes` run two more. Sample
  size and warm-up are each bench's own, the same in both trees.
- **Figures.** Criterion's typical time per iteration (its slope estimate),
  averaged over the rounds. Spread is (max − min) / mean over the rounds.
- **Verdict against 0.25.** A regression when every 0.26 round is worse than
  every 0.25 round and the means differ by more than 5%; an improvement the
  other way; otherwise the same within noise. A 0.26 row with no 0.25 row of
  its name is compared with the closest 0.25 bench, named in the table.
- **`observe_routed` on 0.25.** 0.25 has no such bench. It ran the 0.26 file
  with two changes: the host connects as a 0.25 SDK does (`observe.v1`, no
  `observe.v2`) and answers with a plain `invocationresult`, so the engine
  evaluates the observers itself. That port is not committed.
- **Rust SDK.** `cargo bench -p iii-sdk --bench observe_bench`, three rounds
  per version, interleaved. After the criterion rows, each `call_local/*` case
  runs again paced at 5,000 calls/s for 2 s (the rate of §13), timing each
  call and counting the report items the fake engine receives.
- **Node.** `pnpm bench` in `sdk/packages/node/iii`, three runs. 0.25 has no
  Node bench.
- **Go.** `go test -run '^$' -bench '^BenchmarkIsObserved$' -benchmem -count
  10`. 0.25 has no Go bench.
- **Python.** The Python SDK has no benchmark.
- **Load test.** Two runs per version, in the order 0.25, 0.26, 0.25, 0.26.
  Both run `rpc`, `fanout`, `filters` and `observers`; 0.25's `emit` is 0.26's
  `source`; `match-keys` is 0.26 only. The same rule gives the verdict, with
  higher throughput better and lower memory better.

## §13 budget on the Rust host

Tight-loop figures are criterion means over three rounds; the 5,000 calls/s
figures are means over three rounds of 10,000 calls.

| Scenario | Budget | Measured on 0.26 | 0.25 | Verdict |
|---|---|---|---|---|
| Call, no observers | one atomic load, no allocation, 0 frames | `is_observed/false` 0.436 ns; `call_local/no_observers` 1.9 µs a call; at 5,000 calls/s p50 2.04 µs, p99 6.65 µs, 0.000 report items a call. `is_observed_does_not_allocate` and `a_million_unobserved_local_calls_send_no_frame` pass. | `is_observed/false` 0.495 ns; `trigger_local/no_observers` 2.01 µs | within budget |
| Function matched by keys, observers on other keys (`isObserved(call)` false) | one hash lookup per call key, no allocation | `is_observed_for/key_miss_of_10000` 14.7 ns (one key, 10,000 observers on other keys). `a_key_miss_does_not_allocate` passes. | no equivalent (0.25 has no matching by keys) | within budget |
| Matching by keys, 1 matching observer among 10,000 | ≤ 4 hash lookups + one filter evaluation + 1 report item | `call_local/match_keys_1_of_10000` 3.32 µs a call, against 3.21 µs for a function whose only observer matches; at 5,000 calls/s p50 3.14 µs, 1.000 report items a call. Load test `match-keys`: 897k to 926k calls/s with 100, 1,000 and 10,000 observers. The lookups are not counted: the cost per call does not grow with the observers. | no equivalent | within budget |
| iii's delivery, 10 observers, none matching | < 5 µs (compiled filters) | `call_local/10_observers_none_matching` 2.19 µs a call; at 5,000 calls/s mean 2.95 µs, p90 4.04 µs, p99 7.64 µs (a call with no observers has p99 6.65 µs at the same rate). | `trigger_local/10_observers_none_matching` 2.22 µs | within budget (p99 at 5,000 calls/s is above 5 µs, as it is with no observers) |
| Routed call, observed | 0 extra frames (ids on `invocationresult`) | `observe_routed_bench`: one `invokefunction` and one `invocationresult` a call and no other frame, with 0, 1, 8 and 64 observers, over 4,214,226 calls in 4 rounds; `observe_routed/observed/1` 38.5 µs against 34.9 µs unobserved. Node: the answers to `call_routed/1_observer_matching` carry the observer ids (210,571 `invocationresult` frames with `observed` in run 1), and `bench::one-match` has report items for its local calls only (127,590). | the same frames (the engine evaluates); `observe_routed/observed/1` 37.3 µs | within budget |
| Routed call, `n` observers each filtering on its own value of one path, 1 matching (decision 55) | one index lookup + one filter evaluation, whatever `n` | `observe_routed/filtered/<n>` 37.8 µs a call with 16 observers, 36.5 µs with 4,000; `observe_dispatch/filtered/<n>` 5.25 µs and 6.04 µs; `index/eq_1_of/4000` 211 ns. Measured on `86bebd34b` ("Filters evaluated by the engine"). | `observe_dispatch/filtered/1024` 19.7 µs (a scan) | within budget |
| Event source (handler returns its input), observed | the event serialized once | `call_local/event_source_observed` 3.64 µs a call; at 5,000 calls/s 1.000 report items a call, 1.000 of them carrying the event once (`output_is_input`, no `output`). Node: every `bench::source` item carries it once. | no equivalent (0.25 sources emitted with `obs.emit()`) | within budget |
| Local call to a private function | no copy of input or result when nothing reads the input after the handler; one copy of the input when a hook, a filter or a `capture` reads it | `call_local/private` 1.91 µs (the handler alone: 51.2 ns); `call_local/private_event` 2.21 µs, the same with a deliver hook `call_local/private_deliver_hook` 2.92 µs. `a_local_call_of_a_private_function_copies_nothing` passes (allocations do not grow with the payload). | no equivalent (0.25 has no private functions) | within budget |
| Call through an observer | one routed invocation | No bench times it. `a_call_through_an_observer_is_the_fixture_frame` passes: the call is one `invokefunction` to the observer's target. | no equivalent | within budget (counted by the test) |

## Measurements

The tables of this section are the first pass, on observe-v2 at `e0d87164c`.
"Observer churn fix" measures the observe, control-plane and cleanup benches
and the `fanout`, `filters`, `observers` and `match-keys` load tests again,
on the fix and on observe-v2 before it.

### Observe benches

| Row (0.26) | 0.25 | 0.26 | Δ | Spread 0.25 / 0.26 | Rounds | Verdict | Compared with |
|---|---:|---:|---:|---|---:|---|---|
| `observe_dispatch/default/0` | 1.83 µs | 1.71 µs | -6.8% | 0.4% / 0.7% | 4 / 4 | improvement | `observe_dispatch/auto/0` (1) |
| `observe_dispatch/default/1` | 4.73 µs | 4.7 µs | -0.6% | 1.1% / 1.1% | 4 / 4 | same as 0.25 within noise | `observe_dispatch/auto/1` (1) |
| `observe_dispatch/default/8` | 10.3 µs | 10.4 µs | +0.8% | 1.1% / 1.9% | 4 / 4 | same as 0.25 within noise | `observe_dispatch/auto/8` (1) |
| `observe_dispatch/default/64` | 74.4 µs | 74.5 µs | +0.2% | 1.7% / 1.2% | 4 / 4 | same as 0.25 within noise | `observe_dispatch/auto/64` (1) |
| `observe_dispatch/filtered/16` | 4.78 µs | 4.73 µs | -1.2% | 0.6% / 2.3% | 4 / 4 | same as 0.25 within noise |  |
| `observe_dispatch/filtered/256` | 5.37 µs | 5.43 µs | +1.1% | 1.5% / 3.8% | 4 / 4 | same as 0.25 within noise |  |
| `observe_dispatch/filtered/1024` | 19.7 µs | 19.6 µs | -0.8% | 1.6% / 1.8% | 4 / 4 | same as 0.25 within noise |  |
| `observe_fanout/1` | 11.5 µs | 13.1 µs | +14.5% | 1.0% / 0.2% | 4 / 4 | regression vs 0.25 beyond noise | `trigger_fanout/1` (2) |
| `observe_fanout/8` | 30.1 µs | 31.5 µs | +4.8% | 3.8% / 0.7% | 4 / 4 | same as 0.25 within noise | `trigger_fanout/8` (2) |
| `observe_fanout/32` | 85.4 µs | 66.6 µs | -22.0% | 3.2% / 1.3% | 4 / 4 | improvement | `trigger_fanout/32` (2) |
| `observe_fanout/128` | 321 µs | 176 µs | -45.3% | 5.6% / 0.9% | 4 / 4 | improvement | `trigger_fanout/128` (2) |
| `observe_routed/observed/0` | 35.1 µs | 34.9 µs | -0.4% | 1.1% / 1.0% | 4 / 4 | same as 0.25 within noise | (3) |
| `observe_routed/observed/1` | 37.3 µs | 38.5 µs | +3.2% | 0.5% / 0.3% | 4 / 4 | same as 0.25 within noise | (3) |
| `observe_routed/observed/8` | 45.2 µs | 48.3 µs | +6.9% | 1.5% / 1.2% | 4 / 4 | regression vs 0.25 beyond noise | (3) |
| `observe_routed/observed/64` | 90.7 µs | 99.2 µs | +9.4% | 2.1% / 3.0% | 4 / 4 | regression vs 0.25 beyond noise | (3) |

(1) Same code; 0.25 named the unfiltered case `auto`.  
(2) 0.25 trigger type with the same fan-out and no filter (fire_triggers); 0.26 engine observers of an event source (observers.emit).  
(3) 0.25 run of the same bench (scratch port, not committed): a host without observe.v2, the engine evaluates the observers; 0.26 the host lists them on invocationresult.  

### The other engine benches

| Row (0.26) | 0.25 | 0.26 | Δ | Spread 0.25 / 0.26 | Rounds | Verdict | Compared with |
|---|---:|---:|---:|---|---:|---|---|
| `concurrent_invocation/1` | 2.66 µs | 2.55 µs | -4.0% | 0.2% / 0.5% | 2 / 2 | same as 0.25 within noise |  |
| `concurrent_invocation/8` | 19 µs | 18 µs | -4.9% | 0.1% / 0.2% | 2 / 2 | same as 0.25 within noise |  |
| `concurrent_invocation/32` | 80.9 µs | 76.4 µs | -5.6% | 0.0% / 0.0% | 2 / 2 | improvement |  |
| `concurrent_invocation/128` | 320 µs | 303 µs | -5.3% | 0.4% / 0.2% | 2 / 2 | improvement |  |
| `control_plane_churn/functions_register_remove/100` | 37.7 µs | 37.3 µs | -1.0% | 0.2% / 0.1% | 2 / 2 | same as 0.25 within noise |  |
| `control_plane_churn/functions_register_remove/1000` | 382 µs | 380 µs | -0.6% | 0.1% / 0.2% | 2 / 2 | same as 0.25 within noise |  |
| `control_plane_churn/functions_register_remove/5000` | 1.91 ms | 1.9 ms | -0.6% | 0.2% / 0.0% | 2 / 2 | same as 0.25 within noise |  |
| `control_plane_churn/observers_add_remove/100` | 138 µs | 624 µs | +350.7% | 0.1% / 0.4% | 2 / 2 | regression vs 0.25 | `control_plane_churn/triggers_register_unregister/100` (1) |
| `control_plane_churn/observers_add_remove/1000` | 1.42 ms | 21.5 ms | +1416.9% | 0.1% / 0.1% | 2 / 2 | regression vs 0.25 | `control_plane_churn/triggers_register_unregister/1000` (1) |
| `control_plane_churn/observers_add_remove/5000` | 7.56 ms | 456 ms | +5933.9% | 0.1% / 0.2% | 2 / 2 | regression vs 0.25 | `control_plane_churn/triggers_register_unregister/5000` (1) |
| `control_plane_churn/workers_register_unregister/100` | 380 µs | 357 µs | -6.1% | 0.0% / 0.3% | 2 / 2 | improvement |  |
| `control_plane_churn/workers_register_unregister/1000` | 6.61 ms | 4.23 ms | -36.0% | 1.9% / 0.1% | 2 / 2 | improvement |  |
| `control_plane_churn/workers_register_unregister/5000` | 37.3 ms | 24.1 ms | -35.5% | 3.9% / 1.7% | 2 / 2 | improvement |  |
| `core_runtime/engine_call_registered_handler` | 2.41 µs | 2.26 µs | -5.9% | 0.6% / 0.1% | 2 / 2 | improvement |  |
| `invoke_function_payload_sizes/1kb` | 1.97 µs | 1.84 µs | -6.3% | 0.4% / 0.5% | 4 / 4 | improvement |  |
| `invoke_function_payload_sizes/1mb` | 35.3 µs | 37.1 µs | +5.0% | 13.3% / 4.9% | 4 / 4 | same as 0.25 within noise |  |
| `invoke_function_payload_sizes/10kb` | 2.17 µs | 2.04 µs | -6.0% | 0.8% / 0.4% | 4 / 4 | improvement |  |
| `invoke_function_payload_sizes/100kb` | 6.02 µs | 5.71 µs | -5.2% | 1.8% / 7.4% | 4 / 4 | improvement |  |
| `kv_store/delete` | 825 ns | 819 ns | -0.7% | 0.1% / 0.7% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store/get_hit` | 394 ns | 399 ns | +1.3% | 2.3% / 1.0% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store/get_miss` | 70.9 ns | 69.9 ns | -1.4% | 0.7% / 0.8% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store/list_100_items` | 28.7 µs | 28.7 µs | +0.1% | 0.6% / 0.7% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store/list_groups` | 359 ns | 359 ns | -0.1% | 0.9% / 0.9% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store/list_keys_with_prefix` | 509 ns | 518 ns | +1.6% | 0.0% / 0.5% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store/set_overwrite` | 728 ns | 727 ns | -0.1% | 1.5% / 1.4% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store/update_increment` | 521 ns | 522 ns | +0.3% | 1.3% / 0.2% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store/update_merge` | 1.24 µs | 1.23 µs | -0.8% | 0.1% / 1.2% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store/update_set_field` | 610 ns | 603 ns | -1.2% | 0.8% / 0.9% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store_contention/1` | 415 ns | 416 ns | +0.2% | 1.3% / 1.0% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store_contention/4` | 1.54 µs | 1.54 µs | +0.0% | 0.1% / 1.1% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store_contention/16` | 6.18 µs | 6.18 µs | +0.1% | 1.0% / 0.4% | 2 / 2 | same as 0.25 within noise |  |
| `kv_store_contention/64` | 30 µs | 30.3 µs | +1.0% | 0.3% / 0.0% | 2 / 2 | same as 0.25 within noise |  |
| `metrics_accumulator/existing_function` | 12.2 ns | 11.6 ns | -4.5% | 12.3% / 3.1% | 2 / 2 | same as 0.25 within noise |  |
| `metrics_accumulator/first_invocations` | 7.3 µs | 7.29 µs | -0.1% | 1.4% / 0.4% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_deserialize/InvocationResult` | 1 µs | 995 ns | -0.6% | 1.1% / 1.2% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_deserialize/InvocationResult_Error` | 371 ns | 376 ns | +1.4% | 0.5% / 0.7% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_deserialize/InvokeFunction` | 900 ns | 904 ns | +0.4% | 1.2% / 0.5% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_deserialize/Ping` | 52.2 ns | 48.4 ns | -7.2% | 0.4% / 0.4% | 2 / 2 | improvement |  |
| `protocol_deserialize/Pong` | 52.2 ns | 48.3 ns | -7.3% | 0.4% / 0.3% | 2 / 2 | improvement |  |
| `protocol_deserialize/RegisterFunction` | 406 ns | 418 ns | +2.9% | 0.8% / 0.2% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_deserialize/WorkerRegistered` | 198 ns | 197 ns | -0.5% | 0.4% / 0.8% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_roundtrip/InvocationResult` | 1.31 µs | 1.32 µs | +0.9% | 0.4% / 0.6% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_roundtrip/InvocationResult_Error` | 573 ns | 578 ns | +1.0% | 0.6% / 0.4% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_roundtrip/InvokeFunction` | 1.2 µs | 1.21 µs | +0.8% | 0.5% / 0.6% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_roundtrip/Ping` | 59.3 ns | 59.2 ns | -0.1% | 0.5% / 1.1% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_roundtrip/Pong` | 59.2 ns | 59.4 ns | +0.2% | 0.1% / 0.5% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_roundtrip/RegisterFunction` | 508 ns | 522 ns | +2.8% | 0.7% / 0.4% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_roundtrip/WorkerRegistered` | 335 ns | 333 ns | -0.7% | 2.4% / 0.6% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_serialize/InvocationResult` | 290 ns | 281 ns | -2.9% | 1.3% / 1.6% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_serialize/InvocationResult_Error` | 175 ns | 172 ns | -1.7% | 0.0% / 0.0% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_serialize/InvokeFunction` | 281 ns | 275 ns | -2.2% | 2.4% / 1.9% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_serialize/Ping` | 26.6 ns | 23 ns | -13.3% | 0.1% / 1.1% | 2 / 2 | improvement |  |
| `protocol_serialize/Pong` | 26.8 ns | 23 ns | -13.9% | 0.4% / 0.3% | 2 / 2 | improvement |  |
| `protocol_serialize/RegisterFunction` | 89.2 ns | 86.5 ns | -3.0% | 0.6% / 0.0% | 2 / 2 | same as 0.25 within noise |  |
| `protocol_serialize/WorkerRegistered` | 124 ns | 120 ns | -3.3% | 0.8% / 1.3% | 2 / 2 | same as 0.25 within noise |  |
| `pubsub_fanout/1` | 11 µs | 11.2 µs | +1.6% | 0.5% / 0.8% | 2 / 2 | same as 0.25 within noise |  |
| `pubsub_fanout/8` | 30.9 µs | 29.9 µs | -3.2% | 0.3% / 1.6% | 2 / 2 | same as 0.25 within noise |  |
| `pubsub_fanout/32` | 83 µs | 80.7 µs | -2.7% | 1.8% / 1.4% | 2 / 2 | same as 0.25 within noise |  |
| `pubsub_fanout/128` | 304 µs | 301 µs | -0.9% | 0.3% / 0.3% | 2 / 2 | same as 0.25 within noise |  |
| `startup/build_and_destroy_engine_from_minimal_config` | 511 ms | 510 ms | -0.1% | 0.1% / 0.0% | 2 / 2 | same as 0.25 within noise |  |
| `worker_cleanup/functions/10` | 13.8 µs | 11.5 µs | -16.8% | 5.3% / 3.7% | 2 / 2 | improvement |  |
| `worker_cleanup/functions/50` | 28.2 µs | 27 µs | -4.2% | 0.4% / 0.3% | 2 / 2 | same as 0.25 within noise |  |
| `worker_cleanup/functions/200` | 76.6 µs | 74.1 µs | -3.2% | 2.5% / 0.5% | 2 / 2 | same as 0.25 within noise |  |
| `worker_cleanup/invocations/10` | 8.44 µs | 6.77 µs | -19.8% | 2.5% / 4.3% | 2 / 2 | improvement |  |
| `worker_cleanup/invocations/50` | 7.87 µs | 6.54 µs | -16.9% | 0.6% / 9.0% | 2 / 2 | improvement |  |
| `worker_cleanup/invocations/200` | 8.5 µs | 6.57 µs | -22.7% | 3.9% / 1.0% | 2 / 2 | improvement |  |
| `worker_cleanup/observers/10` | 18.7 µs | 56.6 µs | +203.6% | 0.3% / 1.6% | 2 / 2 | regression vs 0.25 | `worker_cleanup/triggers/10` (2) |
| `worker_cleanup/observers/50` | 50.5 µs | 213 µs | +322.5% | 0.4% / 4.2% | 2 / 2 | regression vs 0.25 | `worker_cleanup/triggers/50` (2) |
| `worker_cleanup/observers/200` | 174 µs | 1.16 ms | +570.9% | 0.3% / 0.1% | 2 / 2 | regression vs 0.25 | `worker_cleanup/triggers/200` (2) |
| `worker_invocation_tracking/add_remove_sequential` | 1.07 µs | 1.07 µs | -0.3% | 0.2% / 0.2% | 2 / 2 | same as 0.25 within noise |  |
| `worker_invocation_tracking/add_with_200_existing` | 1.08 µs | 1.08 µs | +0.1% | 0.3% / 0.3% | 2 / 2 | same as 0.25 within noise |  |
| `worker_invocation_tracking_concurrent/1` | 1.16 µs | 1.15 µs | -0.5% | 0.4% / 0.2% | 2 / 2 | same as 0.25 within noise |  |
| `worker_invocation_tracking_concurrent/8` | 8.63 µs | 8.62 µs | -0.2% | 0.1% / 0.3% | 2 / 2 | same as 0.25 within noise |  |
| `worker_invocation_tracking_concurrent/32` | 37.8 µs | 38 µs | +0.6% | 0.3% / 0.4% | 2 / 2 | same as 0.25 within noise |  |
| `worker_invocation_tracking_concurrent/128` | 152 µs | 153 µs | +0.8% | 0.3% / 0.4% | 2 / 2 | same as 0.25 within noise |  |
| `ws_roundtrip/invoke_echo` | 48.4 µs | 48.9 µs | +0.9% | 0.6% / 1.5% | 2 / 2 | same as 0.25 within noise |  |
| `ws_roundtrip_payload_sizes/1kb` | 50.6 µs | 51 µs | +0.8% | 1.0% / 1.6% | 2 / 2 | same as 0.25 within noise |  |
| `ws_roundtrip_payload_sizes/10kb` | 76.8 µs | 77 µs | +0.2% | 0.1% / 0.5% | 2 / 2 | same as 0.25 within noise |  |
| `ws_roundtrip_payload_sizes/100kb` | 296 µs | 297 µs | +0.4% | 0.5% / 0.6% | 2 / 2 | same as 0.25 within noise |  |

(1) 0.25's bench registers n triggers of one trigger type, each aimed at its own target; 0.26's adds n engine observers of one function, all aimed at one target. A matched bench (one source, n targets, the same observe and unobserve calls) gives 2.9x, 5.3x and 16.5x the 0.25 cost at 100, 1,000 and 5,000, so the shared target is not the cost (see "Check"). With the observer churn fix, 2.3x to 2.4x ("Observer churn fix").  
(2) 0.25 unregisters a worker's n triggers of one trigger type; 0.26 suspends a worker's n observers of one function. The cost per observer is 2.9x to 6.5x the cost per trigger, and 2.7x to 4.0x with the observer churn fix. A bench with n distinct targets was not run.  

### Rust SDK `observe_bench`

| Row (0.26) | 0.25 | 0.26 | Δ | Spread 0.25 / 0.26 | Rounds | Verdict | Compared with |
|---|---:|---:|---:|---|---:|---|---|
| `call_local/1_observer_matching` | 3.38 µs | 3.21 µs | -5.2% | 0.7% / 0.2% | 3 / 3 | improvement | `trigger_local/1_observer_matching` (1) |
| `call_local/10_observers_none_matching` | 2.22 µs | 2.19 µs | -1.6% | 1.9% / 0.8% | 3 / 3 | same as 0.25 within noise | `trigger_local/10_observers_none_matching` (1) |
| `call_local/event_source_observed` | – | 3.64 µs | – | – / 0.3% | 0 / 3 | 0.26 only |  |
| `call_local/match_keys_1_of_10000` | – | 3.32 µs | – | – / 0.5% | 0 / 3 | 0.26 only |  |
| `call_local/no_observers` | 2.01 µs | 1.9 µs | -5.5% | 1.0% / 0.9% | 3 / 3 | improvement | `trigger_local/no_observers` (1) |
| `call_local/private` | – | 1.91 µs | – | – / 1.1% | 0 / 3 | 0.26 only |  |
| `call_local/private_deliver_hook` | – | 2.92 µs | – | – / 0.5% | 0 / 3 | 0.26 only |  |
| `call_local/private_event` | – | 2.21 µs | – | – / 0.4% | 0 / 3 | 0.26 only |  |
| `handler_alone/echo` | – | 51.2 ns | – | – / 3.1% | 0 / 3 | 0.26 only | (2) |
| `is_observed/false` | 0.495 ns | 0.436 ns | -11.8% | 0.5% / 1.0% | 3 / 3 | improvement |  |
| `is_observed/true` | 0.436 ns | 0.44 ns | +1.1% | 0.5% / 3.0% | 3 / 3 | same as 0.25 within noise |  |
| `is_observed_for/key_miss_of_10000` | – | 14.7 ns | – | – / 1.1% | 0 / 3 | 0.26 only |  |

(1) 0.25 trigger() of the same function with the same observers.  
(2) Not compared: 0.25 timed a serde round trip of the input, 0.26 the handler as a call runs it.  

### 0.25 rows with no 0.26 row

| Row (0.25) | 0.25 | Why |
|---|---:|---|
| `handler_alone/echo` | 172 ns | 0.25 timed a serde round trip of the input and output; the 0.26 row of this name times the handler as a call runs it, so the two are not compared |
| `protocol_deserialize/RegisterTrigger` | 565 ns | the `RegisterTrigger` message is gone |
| `protocol_roundtrip/RegisterTrigger` | 756 ns | the `RegisterTrigger` message is gone |
| `protocol_serialize/RegisterTrigger` | 161 ns | the `RegisterTrigger` message is gone |
| `trigger_selection/empty` | 16.6 ns | trigger selection is gone (observers replace triggers) |
| `trigger_selection/matched_8` | 34.7 µs | trigger selection is gone (observers replace triggers) |
| `trigger_selection/mixed_100` | 34.7 µs | trigger selection is gone (observers replace triggers) |
| `trigger_selection/mixed_1000` | 34.8 µs | trigger selection is gone (observers replace triggers) |
| `trigger_selection/no_match_1000` | 16.7 ns | trigger selection is gone (observers replace triggers) |
| `trigger_selection/register_unregister` | 1.2 µs | trigger selection is gone (observers replace triggers) |
| `trigger_selection/single` | 15.4 µs | trigger selection is gone (observers replace triggers) |

### Frames per routed call (`observe_routed_bench`)

Counted at the host for every call of every round, warm-up included.

| Version | Observers | Rounds | Calls | `invokefunction` per call | `invocationresult` per call | Other frames |
|---|---:|---:|---:|---:|---:|---:|
| 0.25 | 0 | 4 | 1,448,434 | 1.000 | 1.000 | 0 |
| 0.25 | 1 | 4 | 1,392,884 | 1.000 | 1.000 | 0 |
| 0.25 | 8 | 4 | 1,251,484 | 1.000 | 1.000 | 0 |
| 0.25 | 64 | 4 | 625,740 | 1.000 | 1.000 | 0 |
| 0.26 | 0 | 4 | 1,448,434 | 1.000 | 1.000 | 0 |
| 0.26 | 1 | 4 | 1,372,684 | 1.000 | 1.000 | 0 |
| 0.26 | 8 | 4 | 933,790 | 1.000 | 1.000 | 0 |
| 0.26 | 64 | 4 | 459,318 | 1.000 | 1.000 | 0 |

### Paced at 5,000 calls/s (Rust SDK `observe_bench`, 0.26)

10,000 calls per round, one at a time on a current-thread runtime. Latency of
each call; the range across rounds in brackets.

| Case | Rounds | Mean | p50 | p90 | p99 | Report items per call | Carrying the value once |
|---|---:|---:|---:|---:|---:|---:|---:|
| `call_local/no_observers` | 3 | 2.54 µs [2.50–2.58] | 2.04 µs [2.04–2.04] | 3.56 µs [3.46–3.67] | 6.65 µs [6.25–7.08] | 0.000 | 0.000 |
| `call_local/private` | 3 | 2.80 µs [2.72–2.84] | 2.12 µs [2.04–2.17] | 4.00 µs [3.62–4.21] | 8.00 µs [7.83–8.25] | 0.000 | 0.000 |
| `call_local/10_observers_none_matching` | 3 | 2.95 µs [2.92–2.98] | 2.39 µs [2.38–2.42] | 4.04 µs [3.92–4.12] | 7.64 µs [7.42–8.04] | 0.000 | 0.000 |
| `call_local/1_observer_matching` | 3 | 3.85 µs [3.63–4.20] | 2.97 µs [2.88–3.12] | 5.18 µs [4.67–5.88] | 10.47 µs [9.04–12.46] | 1.000 | 0.000 |
| `call_local/match_keys_1_of_10000` | 3 | 3.79 µs [3.74–3.83] | 3.14 µs [3.08–3.21] | 5.08 µs [4.92–5.25] | 9.22 µs [8.92–9.42] | 1.000 | 0.000 |
| `call_local/private_event` | 3 | 2.93 µs [2.79–3.15] | 2.24 µs [2.17–2.33] | 4.10 µs [3.83–4.62] | 9.04 µs [7.79–11.21] | 0.000 | 0.000 |
| `call_local/private_deliver_hook` | 3 | 3.75 µs [3.59–3.93] | 3.04 µs [2.96–3.12] | 5.25 µs [4.83–5.71] | 10.04 µs [9.67–10.46] | 0.000 | 0.000 |
| `call_local/event_source_observed` | 3 | 3.93 µs [3.81–4.11] | 3.31 µs [3.25–3.42] | 5.21 µs [4.96–5.62] | 9.40 µs [8.33–10.17] | 1.000 | 1.000 |

### Node SDK (0.26)

| Row | Runs | Mean | Spread | p99 (mean of runs) | Samples per run |
|---|---:|---:|---:|---:|---:|
| `is_observed/false` | 3 | 28.9 ns | 1.3% | 42 ns | 17,318,802 |
| `is_observed/true` | 3 | 29.1 ns | 1.0% | 42 ns | 17,206,843 |
| `is_observed_for/key_miss_of_10000` | 3 | 31.4 ns | 2.0% | 42 ns | 15,942,029 |
| `handler_alone/echo` | 3 | 29.3 ns | 0.9% | 42 ns | 17,092,255 |
| `call_local/no_observers` | 3 | 5.73 µs | 4.7% | 7.81 µs | 174,660 |
| `call_local/private` | 3 | 4.19 µs | 1.8% | 5.6 µs | 238,884 |
| `call_local/10_observers_none_matching` | 3 | 7.49 µs | 2.4% | 9.82 µs | 133,503 |
| `call_local/1_observer_matching` | 3 | 8.44 µs | 2.1% | 12.3 µs | 118,491 |
| `call_local/match_keys_1_of_10000` | 3 | 6.48 µs | 1.6% | 10.5 µs | 154,230 |
| `call_local/event_source_observed` | 3 | 6.27 µs | 0.4% | 9.28 µs | 159,427 |
| `call_local/echo_observed` | 3 | 8.33 µs | 2.6% | 12.3 µs | 120,173 |
| `call_routed/no_observers` | 3 | 4.44 µs | 1.4% | 5.81 µs | 225,280 |
| `call_routed/1_observer_matching` | 3 | 5.18 µs | 0.9% | 6.83 µs | 193,151 |

What the SDK put on the wire, per run (the fake engine's counts):

| Run | `invocationresult` frames | Of them, with `observed` | Report frames | Report items (one-match / keyed / source / echo-observed) | Items carrying the value once (source / echo-observed) |
|---:|---:|---:|---:|---|---|
| 1 | 456,804 | 210,571 | 2,344 | 127,590 / 167,539 / 174,645 / 130,277 | 174,645 / 130,277 |
| 2 | 456,218 | 212,003 | 2,372 | 129,963 / 168,246 / 175,400 / 133,561 | 175,400 / 133,561 |
| 3 | 458,013 | 210,391 | 2,374 | 129,907 / 170,095 / 174,766 / 132,768 | 174,766 / 132,768 |

### Go `BenchmarkIsObserved` (0.26)

| Sub-benchmark | Runs | Mean | Range | B/op | allocs/op |
|---|---:|---:|---|---:|---:|
| `FunctionRef` (GOMAXPROCS 8) | 10 | 1.79 ns | 1.78–1.8 ns | 0 | 0 |
| `ByID` (GOMAXPROCS 8) | 10 | 8.15 ns | 8.11–8.25 ns | 0 | 0 |
| `ByIDParallel` (GOMAXPROCS 8) | 10 | 1.13 ns | 1.07–1.23 ns | 0 | 0 |

### Load test (iii-loadtest `host` profile, Docker linux/arm64)

Two runs per version, in the order 0.25, 0.26, 0.25, 0.26. Each cell lists the
runs. The extra columns read 0.25 → 0.26.


#### `rpc`: calls through the engine, nobody observing

| Step | 0.25 calls/s | 0.26 calls/s | Δ | Verdict | p99 | engine CPU |
|---|---:|---:|---:|---|---:|---:|
| concurrency 1 | 7.97k / 7.33k | 7.4k / 7.33k | -3.7% | same as 0.25 within noise | 151 µs / 160 µs → 155 µs / 160 µs | 0.33 / 0.33 → 0.33 / 0.33 |
| concurrency 8 | 47.1k / 46.3k | 46.5k / 46.3k | -0.7% | same as 0.25 within noise | 251 µs / 254 µs → 252 µs / 253 µs | 1.69 / 1.73 → 1.69 / 1.68 |
| concurrency 32 | 148k / 146k | 146k / 149k | +0.3% | same as 0.25 within noise | 358 µs / 361 µs → 363 µs / 351 µs | 3.28 / 3.26 → 3.26 / 3.28 |
| concurrency 128 | 222k / 224k | 219k / 230k | +0.6% | same as 0.25 within noise | 1.1 ms / 1.1 ms → 1.2 ms / 1.1 ms | 3.50 / 3.51 → 3.49 / 3.55 |
| concurrency 512 | 239k / 236k | 232k / 252k | +1.8% | same as 0.25 within noise | 5.2 ms / 5.2 ms → 5.3 ms / 4.8 ms | 3.48 / 3.48 → 3.46 / 3.55 |
| concurrency 2048 | 195k / 193k | 191k / 213k | +4.1% | same as 0.25 within noise | 26 ms / 26 ms → 26 ms / 22 ms | 3.39 / 3.38 → 3.38 / 3.47 |
| payload 1 KiB | 124k / 125k | 124k / 134k | +3.5% | same as 0.25 within noise | 452 µs / 444 µs → 450 µs / 406 µs | 3.22 / 3.22 → 3.22 / 3.27 |
| payload 16 KiB | 77.5k / 77.6k | 76.9k / 81.5k | +2.1% | same as 0.25 within noise | 748 µs / 745 µs → 751 µs / 727 µs | 3.40 / 3.39 → 3.38 / 3.43 |
| payload 256 KiB | 7.95k / 7.71k | 7.74k / 7.32k | -3.8% | same as 0.25 within noise | 8.4 ms / 8.8 ms → 8.8 ms / 8.8 ms | 2.73 / 2.67 → 2.67 / 2.51 |

#### `fanout`: calls/s with N observers on the called function

| Step | 0.25 calls/s | 0.26 calls/s | Δ | Verdict | engine CPU |
|---|---:|---:|---:|---|---:|
| 0 observers/function | 179k / 178k | 180k / 186k | +2.6% | same as 0.25 within noise | 3.38 / 3.39 → 3.42 / 3.46 |
| 1 observers/function | 100k / 98k | 99.8k / 101k | +1.6% | same as 0.25 within noise | 3.29 / 3.29 → 3.33 / 3.33 |
| 4 observers/function | 55k / 55.2k | 49.8k / 46.2k | -12.9% | regression vs 0.25 beyond noise | 3.40 / 3.40 → 3.41 / 3.35 |
| 16 observers/function | 70.8k / 70.5k | 67.7k / 51.8k | -15.5% | regression vs 0.25 beyond noise | 3.58 / 3.57 → 3.73 / 3.38 |
| 64 observers/function | 68k / 68.1k | 45.8k / 41.7k | -35.8% | regression vs 0.25 beyond noise | 3.63 / 3.55 → 3.55 / 3.46 |

#### `fanout`: activations delivered per second

| Step | 0.25 activations/s | 0.26 activations/s | Δ | Verdict | dropped (OVERFLOW) |
|---|---:|---:|---:|---|---:|
| 0 observers/function | 0 / 0 | 0 / 0 | – | – | 0 / 0 → 0 / 0 |
| 1 observers/function | 76k / 70.9k | 77.8k / 76.9k | +5.4% | improvement | 238k / 269k → 218k / 242k |
| 4 observers/function | 191k / 189k | 195k / 185k | -0.1% | same as 0.25 within noise | 295k / 316k → 44.6k / 2.75k |
| 16 observers/function | 202k / 196k | 232k / 152k | -3.7% | same as 0.25 within noise | 9.28M / 9.29M → 8.49M / 6.73M |
| 64 observers/function | 193k / 190k | 171k / 154k | -15.2% | same as 0.25 within noise over 4 runs per version (see "Check") | 41.5M / 41.6M → 27.4M / 25M |

#### `filters`: calls/s, N filtered observers, each call matches one

| Step | 0.25 calls/s | 0.26 calls/s | Δ | Verdict | engine CPU | p99 |
|---|---:|---:|---:|---|---:|---:|
| 16 filtered observers | 102k / 102k | 98.1k / 98.2k | -3.7% | same as 0.25 within noise | 3.43 / 3.43 → 3.40 / 3.41 | 1.2 ms / 1.2 ms → 1.3 ms / 1.3 ms |
| 256 filtered observers | 95.7k / 95.5k | 88.5k / 87.8k | -7.8% | regression vs 0.25 beyond noise | 3.51 / 3.51 → 3.28 / 3.28 | 1.3 ms / 1.3 ms → 1.4 ms / 1.4 ms |
| 1024 filtered observers | 78.3k / 78k | 30.3k / 30.8k | -60.9% | regression vs 0.25 beyond noise | 3.73 / 3.74 → 1.36 / 1.38 | 1.9 ms / 1.9 ms → 2.4 ms / 2.3 ms |
| 4000 filtered observers | 28.3k / 28.4k | 4.8k / 4.54k | -83.5% | regression vs 0.25 beyond noise | 3.88 / 3.88 → 0.45 / 0.43 | 6.4 ms / 6.5 ms → 14 ms / 15 ms |

#### `filters`: activations delivered per second

| Step | 0.25 activations/s | 0.26 activations/s | Δ | Verdict | dropped (OVERFLOW) |
|---|---:|---:|---:|---|---:|
| 16 filtered observers | 102k / 102k | 98.1k / 98.2k | -3.7% | same as 0.25 within noise | 0 / 0 → 0 / 0 |
| 256 filtered observers | 95.7k / 95.5k | 88.5k / 87.8k | -7.8% | regression vs 0.25 beyond noise | 0 / 0 → 0 / 0 |
| 1024 filtered observers | 33.6k / 33.7k | 30.3k / 30.8k | -9.3% | regression vs 0.25 beyond noise | 391M / 386M → 0 / 0 |
| 4000 filtered observers | 5.11k / 4.89k | 4.8k / 4.54k | -6.5% | regression vs 0.25 beyond noise | 665M / 678M → 0 / 0 |

#### `observers`: creation and disposal rate

| Step | 0.25 per s | 0.26 per s | Δ | Verdict |
|---|---:|---:|---:|---|
| create → 1000 | 24.6k / 26.6k | 43k / 50.6k | +82.7% | improvement |
| create → 4000 | 42.3k / 43.6k | 62.8k / 60.6k | +43.6% | improvement |
| create → 16000 | 4.73k / 4.72k | 4.7k / 4.76k | +0.1% | same as 0.25 within noise |
| create → 64000 | 4.82k / 4.91k | 4.16k / 4.22k | -13.9% | regression vs 0.25 beyond noise |
| dispose 64000 | 4.6k / 4.68k | 4.21k / 4.22k | -9.3% | regression vs 0.25 beyond noise |

#### `observers`: engine memory per observer at rest

| Step | 0.25 per observer | 0.26 per observer | Δ | Verdict |
|---|---:|---:|---:|---|
| 1000 observers (at rest) | 7.18 KiB / 9.70 KiB | 9.55 KiB / 9.62 KiB | +13.6% | same as 0.25 within noise |
| 4000 observers (at rest) | 6.99 KiB / 7.61 KiB | 8.41 KiB / 8.22 KiB | +13.9% | regression vs 0.25 beyond noise |
| 16000 observers (at rest) | 8.00 KiB / 8.38 KiB | 8.28 KiB / 7.91 KiB | -1.1% | same as 0.25 within noise |
| 64000 observers (at rest) | 7.75 KiB / 7.94 KiB | 8.44 KiB / 9.18 KiB | +12.3% | regression vs 0.25 beyond noise |

#### `emit` (0.25) and `source` (0.26): activations delivered per second

| Step | 0.25 activations/s | 0.26 activations/s | Δ | Verdict | dropped (OVERFLOW) |
|---|---:|---:|---:|---|---:|
| 1 observers | 99.9k / 92.6k | 94.4k / 96.9k | -0.6% | same as 0.25 within noise | 104k / 108k → 125k / 127k |
| 16 observers | 199k / 203k | 239k / 243k | +20.0% | improvement | 2.92k / 5.2k → 1.31M / 1.39M |
| 16 observers, 1.0k activations/s offered | 1000 / 999 | 998 / 998 | -0.1% | same as 0.25 within noise | 0 / 0 → 0 / 0 |
| 16 observers, 5.0k activations/s offered | 5k / 5k | 4.99k / 4.99k | -0.0% | same as 0.25 within noise | 0 / 0 → 0 / 0 |
| 16 observers, 20k activations/s offered | 20k / 20k | 20k / 20k | -0.0% | same as 0.25 within noise | 0 / 0 → 0 / 0 |
| 16 observers, 50k activations/s offered | 50k / 50k | 49.9k / 50k | -0.0% | same as 0.25 within noise | 0 / 0 → 0 / 0 |
| 16 observers, 100k activations/s offered | 99.9k / 100k | 99.9k / 99.9k | -0.0% | same as 0.25 within noise | 0 / 0 → 0 / 0 |
| 16 observers, 200k activations/s offered | 114k / 115k | 200k / 200k | +74.4% | improvement | 0 / 0 → 0 / 0 |
| 16 observers, 500k activations/s offered | – | 334k / 333k | – | 0.26 only | – → 1.01M / 883k |
| 256 observers | 157k / 153k | 237k / 212k | +44.8% | improvement | 19 / 58 → 3.64M / 3.22M |

#### `match-keys` (0.26 only): calls/s

| Step | 0.25 calls/s | 0.26 calls/s | Δ | Verdict | engine CPU | p99 |
|---|---:|---:|---:|---|---:|---:|
| 100 keyed observers, 1 matching | – | 897k / 925k | – | 0.26 only | – → 2.99 / 2.96 | – → 14 µs / 15 µs |
| 100 keyed observers, none matching | – | 1.85M / 1.92M | – | 0.26 only | – → 0.00 / 0.00 | – → 9 µs / 9 µs |
| 1000 keyed observers, 1 matching | – | 918k / 926k | – | 0.26 only | – → 2.95 / 3.01 | – → 15 µs / 16 µs |
| 1000 keyed observers, none matching | – | 1.92M / 1.87M | – | 0.26 only | – → 0.00 / 0.00 | – → 9 µs / 9 µs |
| 10000 keyed observers, 1 matching | – | 912k / 910k | – | 0.26 only | – → 2.90 / 2.85 | – → 17 µs / 18 µs |
| 10000 keyed observers, none matching | – | 1.91M / 1.91M | – | 0.26 only | – → 0.01 / 0.01 | – → 9 µs / 9 µs |

#### `match-keys` (0.26 only): activations delivered per second

| Step | 0.25 activations/s | 0.26 activations/s | Δ | Verdict |
|---|---:|---:|---:|---|
| 100 keyed observers, 1 matching | – | 143k / 138k | – | 0.26 only |
| 100 keyed observers, none matching | – | 0 / 0 | – | 0.26 only |
| 1000 keyed observers, 1 matching | – | 137k / 134k | – | 0.26 only |
| 1000 keyed observers, none matching | – | 0 / 0 | – | 0.26 only |
| 10000 keyed observers, 1 matching | – | 83.8k / 78.9k | – | 0.26 only |
| 10000 keyed observers, none matching | – | 0 / 0 | – | 0.26 only |

## Observer churn fix

`ov2/observer-churn` at `d37f78f42` keeps a function's active observers, in
the engine and in a Rust host's compiled set, in persistent sets of 128-slot
nodes, so adding, removing or suspending one observer of a function with n
observers copies O(log n) slots. This pass measured it on 2026-10-31, on the
machine of the first pass, against observe-v2 before it (`b05994f93`, whose
engine, Rust SDK and load test are those of the first pass's `e0d87164c`) and
against 0.25 (`b0b86fd37`). The Rust SDK, Node and Go benches and the §13
budget were not measured again.

### Method

- **Engine benches.** `control_plane_churn`, `worker_cleanup`,
  `observe_dispatch`, `observe_fanout` (0.25: `trigger_fanout`) and
  `observe_routed` (0.25: the first pass's scratch port), bench by bench in
  the order 0.25, before the fix, the fix, in two rounds (`--save-baseline f1`
  and `f2`), each version in its own target directory. Figures, spread and
  verdicts as in the first pass.
- **Matched shapes.** A scratch bench (not committed) adds n engine observers
  with `observe_as_engine` and removes them with `unobserve_as_engine`, as
  `observers_add_remove` does, in three shapes: one source and one target (the
  committed bench), one source and n targets (0.25's shape: one trigger type,
  n target functions), and n sources sharing one target; the n sources and n
  targets are function ids nobody registers. It ran before the
  fix, then on the fix, after each engine round, and is compared with 0.25's
  `triggers_register_unregister` of the same rounds.
- **Load test.** The first pass's image, CPU sets, memory cap and flags, with
  `--port 58200`: `fanout`, `filters` and `observers` for every version,
  `match-keys` for 0.26. Runs in the order before, fix, 0.25, fix, before;
  `fanout` and `match-keys`, whose runs spread most, ran twice more per
  version in the order fix, before, before, fix. The verdicts against 0.25 use
  this pass's 0.25 run and the first pass's two.
- **Machine.** One unrelated process at 100% of one core and a browser
  renderer at 30% to 50% of one throughout; load average 5.0 to 12.3 (its
  readings count each bench's own threads). This pass's 0.25 load-test run is
  within 4% of the first pass's mean at every step but five: `fanout` with 4
  observers (+17% calls/s), creating up to 4,000 observers (−24%), creating
  4,000 filtered observers (−7.5%), and memory at rest with 1,000 observers
  (−15%, within the first pass's range) and with 4,000 (+21%).

### What changed

- **Adding and removing observers of one function.** One add and remove costs
  3.97 µs with 100 observers, 4.34 µs with 1,000 and 4.92 µs with 5,000
  (6.26, 21.5 and 90.9 µs before the fix): the cost of one change grows by 24%
  from 100 to 5,000 observers. 0.25's triggers cost 1.38 to 1.50 µs each, so
  the fix costs 2.9x, 3.1x and 3.3x. In the matched bench, one source with n
  targets costs 3.2 to 3.6 µs a change (2.3x to 2.4x 0.25), n sources 3.4 to
  3.8 µs (2.5x), and a target shared by all n observers 0.7 to 1.4 µs more a
  change than n targets, more as n grows.
- **A worker's cleanup.** Suspending a worker's observers of one function
  costs 4.97, 3.46 and 3.48 µs per observer with 10, 50 and 200 (5.49, 4.16
  and 5.73 µs before the fix), against 1.86, 0.99 and 0.86 µs per trigger in
  0.25: 2.7x, 3.5x and 4.0x.
- **The other engine rows** are the same as before the fix within noise
  (within 2.2% either side).
- **Load test, creating and disposing of observers.** The `observers`
  scenario creates 64,000 observers at 16.3k a second and disposes of them at
  10.4k to 10.6k, against 4.2k to 4.3k for each before the fix and 4.8k to
  4.9k and 4.5k to 4.7k in 0.25. `filters` creates 4,000 filtered observers at
  80k to 96k a second (6.0k before the fix, 12k to 13.5k in 0.25). The
  `match-keys` worker creates its keyed observers of one function at 50k to
  58k a second up to 100, 53k to 71k up to 1,000 and 4.1k up to 10,000 (45k
  to 48k, 18k to 19k and 1.5k before the fix). The step to 10,000 opens nine
  more worker connections of 1,000 observers each and takes 2.2 s; the
  `observers` step to 16,000, which opens twelve, takes 2.2 to 2.6 s in every
  version (4.6k to 5.5k observers a second).
- **Load test, calls.** `fanout` with 64 observers a function takes 20.6% more
  calls per second and delivers 20.7% more activations than before the fix
  (four runs each); with 0, 1, 4 and 16 observers the two are the same within
  noise. `filters` and `match-keys` calls and activations are the same as
  before the fix within noise.
- **Load test, memory.** Engine memory per observer at rest is 9.1 KiB with
  4,000 and 16,000 observers and 9.7 KiB with 64,000, against 8.2, 8.1 and 8.9
  KiB before the fix in the same pass (+11.6%, +12.8%, +8.5%) and 7.8, 8.1
  and 7.8 KiB in 0.25 (+16.8%, +11.9%, +24.0%). With 1,000 observers, runs of
  every version measure about 7.1 or about 9.7 KiB; the fix's two measured 9.6
  and 9.7.

### Engine benches

Two rounds per version; Δ and spread as in the first pass.

| Row | 0.25 | Before | Fix | Δ fix vs before | Δ fix vs 0.25 | Δ before vs 0.25 | Spread 0.25 / before / fix | Verdict, fix vs before | Verdict, fix vs 0.25 |
|---|---:|---:|---:|---:|---:|---:|---|---|---|
| `control_plane_churn/functions_register_remove/100` | 37.6 µs | 37.5 µs | 37.5 µs | +0.0% | -0.4% | -0.5% | 0.1% / 0.5% / 0.1% | same within noise | same within noise |
| `control_plane_churn/functions_register_remove/1000` | 382 µs | 380 µs | 381 µs | +0.1% | -0.3% | -0.4% | 0.5% / 0.3% / 0.1% | same within noise | same within noise |
| `control_plane_churn/functions_register_remove/5000` | 1.91 ms | 1.89 ms | 1.89 ms | +0.0% | -0.8% | -0.8% | 0.2% / 0.5% / 0.1% | same within noise | same within noise |
| `control_plane_churn/observers_add_remove/100` (1) | 138 µs | 626 µs | 397 µs | -36.6% | +186.8% | +352.2% | 0.2% / 0.6% / 2.1% | faster | regression |
| `control_plane_churn/observers_add_remove/1000` (1) | 1.41 ms | 21.5 ms | 4.34 ms | -79.8% | +207.1% | +1422.3% | 0.0% / 0.4% / 0.8% | faster | regression |
| `control_plane_churn/observers_add_remove/5000` (1) | 7.52 ms | 454 ms | 24.6 ms | -94.6% | +226.7% | +5939.9% | 0.1% / 0.2% / 0.5% | faster | regression |
| `control_plane_churn/workers_register_unregister/100` | 380 µs | 356 µs | 359 µs | +0.8% | -5.4% | -6.1% | 0.2% / 0.0% / 1.6% | same within noise | improvement |
| `control_plane_churn/workers_register_unregister/1000` | 6.39 ms | 4.19 ms | 4.21 ms | +0.5% | -34.1% | -34.4% | 0.9% / 0.1% / 1.0% | same within noise | improvement |
| `control_plane_churn/workers_register_unregister/5000` | 36.1 ms | 23.6 ms | 23.7 ms | +0.3% | -34.4% | -34.5% | 1.0% / 0.4% / 1.3% | same within noise | improvement |
| `observe_dispatch/default/0` (2) | 1.83 µs | 1.71 µs | 1.71 µs | +0.2% | -6.4% | -6.5% | 0.2% / 0.4% / 0.5% | same within noise | improvement |
| `observe_dispatch/default/1` (2) | 4.73 µs | 4.69 µs | 4.73 µs | +0.8% | -0.1% | -0.9% | 0.1% / 0.8% / 0.3% | same within noise | same within noise |
| `observe_dispatch/default/8` (2) | 10.6 µs | 10.6 µs | 10.5 µs | -0.9% | -1.4% | -0.5% | 1.1% / 0.9% / 0.7% | same within noise | same within noise |
| `observe_dispatch/default/64` (2) | 75.7 µs | 75.4 µs | 75 µs | -0.5% | -0.9% | -0.4% | 1.1% / 0.3% / 0.9% | same within noise | same within noise |
| `observe_dispatch/filtered/16` | 4.77 µs | 4.73 µs | 4.71 µs | -0.3% | -1.3% | -1.0% | 1.4% / 0.6% / 0.0% | same within noise | same within noise |
| `observe_dispatch/filtered/256` | 5.32 µs | 5.38 µs | 5.37 µs | -0.3% | +0.9% | +1.2% | 2.6% / 2.9% / 0.1% | same within noise | same within noise |
| `observe_dispatch/filtered/1024` | 19.8 µs | 19.6 µs | 20 µs | +1.8% | +1.1% | -0.7% | 0.6% / 0.8% / 0.4% | same within noise | same within noise |
| `observe_fanout/1` (3) | 11.4 µs | 13.1 µs | 13.1 µs | +0.2% | +14.9% | +14.6% | 0.8% / 0.5% / 0.2% | same within noise | regression |
| `observe_fanout/8` (3) | 28.7 µs | 30.3 µs | 30.6 µs | +0.9% | +6.4% | +5.5% | 0.3% / 1.0% / 0.8% | same within noise | regression |
| `observe_fanout/32` (3) | 84.7 µs | 66.9 µs | 67 µs | +0.2% | -20.9% | -21.0% | 0.5% / 0.3% / 0.3% | same within noise | improvement |
| `observe_fanout/128` (3) | 321 µs | 176 µs | 176 µs | -0.1% | -45.1% | -45.0% | 1.2% / 0.3% / 0.1% | same within noise | improvement |
| `observe_routed/observed/0` | 34.5 µs | 34.5 µs | 34.8 µs | +0.8% | +0.8% | +0.1% | 0.8% / 1.6% / 1.0% | same within noise | same within noise |
| `observe_routed/observed/1` | 36.7 µs | 38.2 µs | 38.1 µs | -0.2% | +3.9% | +4.1% | 0.2% / 1.0% / 0.2% | same within noise | same within noise |
| `observe_routed/observed/8` | 44.2 µs | 47.6 µs | 47.4 µs | -0.5% | +7.3% | +7.8% | 0.6% / 0.7% / 0.2% | same within noise | regression |
| `observe_routed/observed/64` | 93.4 µs | 102 µs | 101 µs | -0.7% | +8.1% | +8.9% | 2.0% / 0.0% / 0.5% | same within noise | regression |
| `worker_cleanup/functions/10` | 13.2 µs | 11.3 µs | 11.3 µs | +0.3% | -14.5% | -14.7% | 1.4% / 2.3% / 1.0% | same within noise | improvement |
| `worker_cleanup/functions/50` | 27.8 µs | 26.7 µs | 27.3 µs | +2.2% | -2.1% | -4.2% | 1.8% / 3.5% / 0.8% | same within noise | same within noise |
| `worker_cleanup/functions/200` | 76.5 µs | 75.1 µs | 76 µs | +1.3% | -0.6% | -1.9% | 4.5% / 1.3% / 1.1% | same within noise | same within noise |
| `worker_cleanup/invocations/10` | 8.3 µs | 6.76 µs | 6.85 µs | +1.3% | -17.5% | -18.6% | 1.6% / 0.6% / 2.3% | same within noise | improvement |
| `worker_cleanup/invocations/50` | 7.82 µs | 5.96 µs | 5.95 µs | -0.1% | -23.9% | -23.8% | 4.0% / 0.5% / 1.4% | same within noise | improvement |
| `worker_cleanup/invocations/200` | 8.51 µs | 6.43 µs | 6.36 µs | -1.1% | -25.3% | -24.5% | 6.4% / 4.4% / 1.7% | same within noise | improvement |
| `worker_cleanup/observers/10` (4) | 18.6 µs | 54.9 µs | 49.7 µs | -9.5% | +167.5% | +195.8% | 1.9% / 0.1% / 3.0% | faster | regression |
| `worker_cleanup/observers/50` (4) | 49.7 µs | 208 µs | 173 µs | -16.9% | +248.0% | +318.5% | 0.3% / 1.3% / 2.2% | faster | regression |
| `worker_cleanup/observers/200` (4) | 173 µs | 1.15 ms | 696 µs | -39.2% | +303.0% | +562.9% | 0.6% / 2.1% / 1.7% | faster | regression |

(1) 0.25: `control_plane_churn/triggers_register_unregister/<n>`, a different shape (the first pass's notes).  
(2) 0.25: `observe_dispatch/auto/<n>`.  
(3) 0.25: `trigger_fanout/<n>`.  
(4) 0.25: `worker_cleanup/triggers/<n>`, a different shape (the first pass's notes).

### One change, per observer

The rows of `observers_add_remove` and `worker_cleanup/observers` above, divided by n.

| Bench | n | 0.25, per trigger | Before | Fix | Fix / 0.25 |
|---|---:|---:|---:|---:|---:|
| `control_plane_churn/observers_add_remove` | 100 | 1.38 µs | 6.26 µs | 3.97 µs | 2.9x |
| `control_plane_churn/observers_add_remove` | 1,000 | 1.41 µs | 21.52 µs | 4.34 µs | 3.1x |
| `control_plane_churn/observers_add_remove` | 5,000 | 1.50 µs | 90.89 µs | 4.92 µs | 3.3x |
| `worker_cleanup/observers` | 10 | 1.86 µs | 5.49 µs | 4.97 µs | 2.7x |
| `worker_cleanup/observers` | 50 | 0.99 µs | 4.16 µs | 3.46 µs | 3.5x |
| `worker_cleanup/observers` | 200 | 0.86 µs | 5.73 µs | 3.48 µs | 4.0x |

### Matched shapes (scratch bench, not committed)

Two rounds per version. The 0.25 column is `control_plane_churn/triggers_register_unregister/<n>` of the engine rounds above.

| Shape | n | Before | Fix | Fix vs before | Before, per add and remove | Fix, per add and remove | Before / 0.25 | Fix / 0.25 | Spread before / fix |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| `one_source_one_target` | 100 | 621 µs | 394 µs | -36.6% | 6.21 µs | 3.94 µs | 4.5x | 2.8x | 0.6% / 0.1% |
| `one_source_one_target` | 1,000 | 21.5 ms | 4.34 ms | -79.8% | 21.49 µs | 4.34 µs | 15.2x | 3.1x | 0.7% / 4.6% |
| `one_source_one_target` | 5,000 | 451 ms | 24.6 ms | -94.5% | 90.29 µs | 4.92 µs | 60.0x | 3.3x | 1.1% / 2.5% |
| `one_source_n_targets` | 100 | 406 µs | 323 µs | -20.5% | 4.06 µs | 3.23 µs | 2.9x | 2.3x | 0.1% / 0.3% |
| `one_source_n_targets` | 1,000 | 7.52 ms | 3.22 ms | -57.1% | 7.52 µs | 3.22 µs | 5.3x | 2.3x | 0.5% / 0.3% |
| `one_source_n_targets` | 5,000 | 128 ms | 17.8 ms | -86.0% | 25.55 µs | 3.57 µs | 17.0x | 2.4x | 0.1% / 1.6% |
| `n_sources_one_target` | 100 | 385 µs | 341 µs | -11.4% | 3.85 µs | 3.41 µs | 2.8x | 2.5x | 0.7% / 0.5% |
| `n_sources_one_target` | 1,000 | 3.86 ms | 3.48 ms | -9.7% | 3.86 µs | 3.48 µs | 2.7x | 2.5x | 0.5% / 0.6% |
| `n_sources_one_target` | 5,000 | 20.8 ms | 18.9 ms | -9.0% | 4.16 µs | 3.79 µs | 2.8x | 2.5x | 0.3% / 1.3% |

### Load test

Each cell lists the runs. The 0.25 column gives this pass's run, then the
first pass's two in brackets; the extra column reads 0.25 → before → fix
(before → fix for `match-keys`).

#### `fanout`: calls/s with N observers on the called function

| Step | 0.25: this pass (first pass) | Before | Fix | Δ vs before | Verdict vs before | Δ vs 0.25 | Verdict vs 0.25 | engine CPU |
|---|---:|---:|---:|---:|---|---:|---|---:|
| 0 observers/function | 178k (179k / 178k) | 186k / 191k / 181k / 191k | 182k / 186k / 185k / 177k | -2.6% | same within noise | +2.2% | same within noise | 3.37 (3.38 / 3.39) → 3.44 / 3.49 / 3.40 / 3.48 → 3.43 / 3.44 / 3.44 / 3.37 |
| 1 observers/function | 97.9k (100k / 98k) | 101k / 99.6k / 101k / 101k | 99.3k / 100k / 101k / 99.9k | -0.7% | same within noise | +1.4% | same within noise | 3.28 (3.29 / 3.29) → 3.32 / 3.34 / 3.33 / 3.32 → 3.32 / 3.34 / 3.33 / 3.33 |
| 4 observers/function | 64.5k (55k / 55.2k) | 48.5k / 47.8k / 46k / 48.4k | 50.4k / 49.4k / 47.6k / 46.3k | +1.6% | same within noise | -16.8% | regression | 3.48 (3.40 / 3.40) → 3.38 / 3.40 / 3.36 / 3.31 → 3.43 / 3.43 / 3.37 / 3.37 |
| 16 observers/function | 70.4k (70.8k / 70.5k) | 60.2k / 62.3k / 64.2k / 66.8k | 57.1k / 52.1k / 67.5k / 66.2k | -4.1% | same within noise | -14.0% | regression | 3.57 (3.58 / 3.57) → 3.53 / 3.61 / 3.62 / 3.68 → 3.51 / 3.40 / 3.70 / 3.66 |
| 64 observers/function | 68.1k (68k / 68.1k) | 39.2k / 38.5k / 36.5k / 42.5k | 45.8k / 45.3k / 45.9k / 51.9k | +20.6% | better | -30.6% | regression | 3.55 (3.63 / 3.55) → 3.40 / 3.39 / 3.33 / 3.55 → 3.54 / 3.55 / 3.53 / 3.68 |

#### `fanout`: activations delivered per second

| Step | 0.25: this pass (first pass) | Before | Fix | Δ vs before | Verdict vs before | Δ vs 0.25 | Verdict vs 0.25 | dropped (OVERFLOW) |
|---|---:|---:|---:|---:|---|---:|---|---:|
| 1 observers/function | 70.9k (76k / 70.9k) | 80.4k / 81.5k / 74.7k / 77.5k | 77.8k / 81.2k / 77.9k / 75.4k | -0.6% | same within noise | +7.6% | same within noise | 268k (238k / 269k) → 208k / 179k / 259k / 233k → 214k / 187k / 227k / 243k |
| 4 observers/function | 196k (191k / 189k) | 188k / 190k / 184k / 191k | 194k / 196k / 190k / 185k | +1.6% | same within noise | -0.3% | same within noise | 614k (295k / 316k) → 56k / 9.8k / 1.72k / 22k → 73.1k / 11.1k / 7.03k / 2.96k |
| 16 observers/function | 194k (202k / 196k) | 178k / 196k / 194k / 211k | 168k / 157k / 231k / 207k | -2.1% | same within noise | -3.6% | same within noise | 9.29M (9.28M / 9.29M) → 7.82M / 7.98M / 8.3M / 8.54M → 7.43M / 6.74M / 8.47M / 8.48M |
| 64 observers/function | 186k (193k / 190k) | 146k / 140k / 141k / 157k | 171k / 168k / 172k / 195k | +20.7% | better | -6.9% | same within noise | 41.6M (41.5M / 41.6M) → 23.5M / 23.1M / 21.8M / 25.5M → 27.5M / 27.2M / 27.5M / 31.2M |

#### `filters`: calls/s, N filtered observers, each call matches one

| Step | 0.25: this pass (first pass) | Before | Fix | Δ vs before | Verdict vs before | Δ vs 0.25 | Verdict vs 0.25 | engine CPU |
|---|---:|---:|---:|---:|---|---:|---|---:|
| 16 filtered observers | 102k (102k / 102k) | 96.8k / 98.8k | 95k / 98k | -1.3% | same within noise | -5.3% | regression | 3.43 (3.43 / 3.43) → 3.39 / 3.41 → 3.39 / 3.40 |
| 256 filtered observers | 96.3k (95.7k / 95.5k) | 87.3k / 87.4k | 85.6k / 86.9k | -1.3% | same within noise | -10.0% | regression | 3.51 (3.51 / 3.51) → 3.24 / 3.26 → 3.23 / 3.24 |
| 1024 filtered observers | 77.5k (78.3k / 78k) | 30.6k / 30.7k | 30.7k / 31k | +0.7% | same within noise | -60.4% | regression | 3.74 (3.73 / 3.74) → 1.37 / 1.37 → 1.38 / 1.37 |
| 4000 filtered observers | 28.1k (28.3k / 28.4k) | 4.76k / 4.47k | 4.61k / 4.53k | -1.0% | same within noise | -83.8% | regression | 3.89 (3.88 / 3.88) → 0.43 / 0.40 → 0.46 / 0.43 |

#### `filters`: activations delivered per second

| Step | 0.25: this pass (first pass) | Before | Fix | Δ vs before | Verdict vs before | Δ vs 0.25 | Verdict vs 0.25 | dropped (OVERFLOW) |
|---|---:|---:|---:|---:|---|---:|---|---:|
| 16 filtered observers | 102k (102k / 102k) | 96.8k / 98.8k | 95k / 98k | -1.3% | same within noise | -5.3% | regression | 0 (0 / 0) → 0 / 0 → 0 / 0 |
| 256 filtered observers | 96.3k (95.7k / 95.5k) | 87.3k / 87.4k | 85.6k / 86.9k | -1.3% | same within noise | -10.0% | regression | 0 (0 / 0) → 0 / 0 → 0 / 0 |
| 1024 filtered observers | 33.7k (33.6k / 33.7k) | 30.6k / 30.7k | 30.7k / 31k | +0.7% | same within noise | -8.3% | regression | 382M (391M / 386M) → 0 / 0 → 0 / 0 |
| 4000 filtered observers | 4.91k (5.11k / 4.89k) | 4.76k / 4.47k | 4.61k / 4.52k | -0.9% | same within noise | -8.0% | regression | 666M (665M / 678M) → 0 / 0 → 0 / 0 |

#### `filters`: observers created per second, up to N

| Step | 0.25: this pass (first pass) | Before | Fix | Δ vs before | Verdict vs before | Δ vs 0.25 | Verdict vs 0.25 |
|---|---:|---:|---:|---:|---|---:|---|
| 16 filtered observers | 14.3k (16.4k / 12.9k) | 23k / 15.7k | 16.4k / 19.2k | -8.2% | same within noise | +22.3% | same within noise |
| 256 filtered observers | 28.8k (29.3k / 26.1k) | 27.6k / 17.2k | 38.3k / 29k | +50.1% | better | +19.9% | same within noise |
| 1024 filtered observers | 19.8k (22.4k / 17.2k) | 18.7k / 17k | 37.2k / 51.3k | +147.7% | better | +123.5% | improvement |
| 4000 filtered observers | 12.3k (13.5k / 13k) | 6.04k / 6.02k | 79.5k / 95.8k | +1354.3% | better | +578.3% | improvement |

#### `observers`: creation and disposal rate (per second)

| Step | 0.25: this pass (first pass) | Before | Fix | Δ vs before | Verdict vs before | Δ vs 0.25 | Verdict vs 0.25 |
|---|---:|---:|---:|---:|---|---:|---|
| create → 1000 | 26k (24.6k / 26.6k) | 46k / 55k | 46.8k / 45.2k | -8.8% | same within noise | +78.9% | improvement |
| create → 4000 | 32.8k (42.3k / 43.6k) | 65.6k / 45.5k | 69.2k / 79.4k | +33.8% | better | +87.9% | improvement |
| create → 16000 | 4.75k (4.73k / 4.72k) | 4.76k / 4.56k | 5.43k / 5.47k | +17.0% | better | +15.2% | improvement |
| create → 64000 | 4.79k (4.82k / 4.91k) | 4.26k / 4.2k | 16.3k / 16.3k | +285.9% | better | +237.2% | improvement |
| dispose 64000 | 4.5k (4.6k / 4.68k) | 4.32k / 4.19k | 10.4k / 10.6k | +147.4% | better | +129.1% | improvement |

#### `observers`: engine memory per observer at rest (KiB)

| Step | 0.25: this pass (first pass) | Before | Fix | Δ vs before | Verdict vs before | Δ vs 0.25 | Verdict vs 0.25 |
|---|---:|---:|---:|---:|---|---:|---|
| 1000 observers (at rest) | 7.19 (7.18 / 9.70) | 7.11 / 7.09 | 9.71 / 9.61 | +36.1% | worse | +20.4% | same within noise |
| 4000 observers (at rest) | 8.80 (6.99 / 7.61) | 7.43 / 8.89 | 9.22 / 8.99 | +11.6% | worse | +16.8% | regression |
| 16000 observers (at rest) | 8.03 (8.00 / 8.38) | 7.90 / 8.25 | 9.53 / 8.68 | +12.8% | worse | +11.9% | regression |
| 64000 observers (at rest) | 7.67 (7.75 / 7.94) | 8.90 / 8.90 | 9.73 / 9.59 | +8.5% | worse | +24.0% | regression |

#### `match-keys` (0.26 only): observers created per second, up to N

| Step | Before | Fix | Δ vs before | Verdict vs before |
|---|---:|---:|---:|---|
| 100 keyed observers, 1 matching | 45.6k / 46.6k / 47.9k / 45.3k | 49.8k / 50.4k / 57.5k / 56.3k | +15.4% | better |
| 1000 keyed observers, 1 matching | 19k / 18.7k / 17.9k / 18.2k | 66.2k / 52.6k / 58.1k / 70.6k | +235.4% | better |
| 10000 keyed observers, 1 matching | 1.49k / 1.5k / 1.49k / 1.5k | 4.12k / 4.13k / 4.08k / 4.08k | +174.3% | better |

#### `match-keys` (0.26 only): calls/s

| Step | Before | Fix | Δ vs before | Verdict vs before | engine CPU |
|---|---:|---:|---:|---|---:|
| 100 keyed observers, 1 matching | 980k / 916k / 936k / 934k | 962k / 928k / 925k / 952k | +0.0% | same within noise | 2.97 / 2.99 / 2.98 / 2.95 → 2.99 / 2.98 / 2.97 / 2.96 |
| 100 keyed observers, none matching | 1.95M / 2.19M / 2.03M / 1.94M | 2.1M / 1.95M / 2.16M / 2.06M | +2.1% | same within noise | 0.00 / 0.00 / 0.00 / 0.00 → 0.00 / 0.00 / 0.00 / 0.00 |
| 1000 keyed observers, 1 matching | 948k / 906k / 923k / 861k | 958k / 931k / 894k / 925k | +2.0% | same within noise | 2.95 / 2.96 / 2.98 / 3.02 → 2.84 / 2.93 / 2.98 / 2.97 |
| 1000 keyed observers, none matching | 2.05M / 2.11M / 2.04M / 1.89M | 2.07M / 1.9M / 1.93M / 2.1M | -1.1% | same within noise | 0.00 / 0.00 / 0.00 / 0.00 → 0.00 / 0.00 / 0.00 / 0.00 |
| 10000 keyed observers, 1 matching | 940k / 926k / 919k / 873k | 918k / 921k / 861k / 936k | -0.6% | same within noise | 2.85 / 2.77 / 2.85 / 2.86 → 2.80 / 2.84 / 2.82 / 2.83 |
| 10000 keyed observers, none matching | 1.93M / 2.16M / 2.07M / 1.87M | 2.01M / 1.95M / 1.94M / 2.14M | -0.1% | same within noise | 0.46 / 0.01 / 0.01 / 0.00 → 0.01 / 0.01 / 0.01 / 0.01 |

#### `match-keys` (0.26 only): activations delivered per second

| Step | Before | Fix | Δ vs before | Verdict vs before |
|---|---:|---:|---:|---|
| 100 keyed observers, 1 matching | 133k / 140k / 139k / 139k | 131k / 138k / 140k / 128k | -2.6% | same within noise |
| 1000 keyed observers, 1 matching | 132k / 135k / 135k / 137k | 87k / 117k / 138k / 135k | -11.4% | same within noise |
| 10000 keyed observers, 1 matching | 73.1k / 69.7k / 76.1k / 75.5k | 77.8k / 74.6k / 75.2k / 65.2k | -0.6% | same within noise |

## Filters evaluated by the engine (decision 55)

Decision 55 evaluates the filters of every routed call in the engine and
matches each evaluation through `iii_filter::FilterIndex`. Measured on
`ov2/filters-engine` at `86bebd34b` against observe-v2 at `edc636d13`, on the
machine of "Setup", with other agents' builds and benches running (load
average 7 to 18). Engine benches: `cargo bench -p iii --bench <name> --
--save-baseline rN --noplot`, the two versions alternating bench by bench,
two rounds (four for `control_plane_churn` and `observe_fanout`). Figures are
criterion's typical time averaged over the rounds, and the verdict is the
rule of "Method".

### Where the time went

The `filters` load test calls one function, hosted by one Rust worker, with
N observers `{"input.k": {"$eq": i}}`; each call matches one. On observe-v2
the host evaluated every observer's filter for each routed call (the engine
was mostly idle), and the engine ran the same scan for hosts without
`observe.v2`:

| On observe-v2 (`edc636d13`) | 1,024 observers | 4,000 observers |
|---|---:|---:|
| Rust host, one call through iii's delivery (`call_local/filtered_1_of_<n>`, a scratch row of `observe_bench`, not committed: the scan a routed call ran too) | 23.3 µs | 82.7 µs |
| Engine evaluation (`observe_dispatch/filtered/<n>`) | 19.7 µs | 68.1 µs |
| `iii-filter`, every filter evaluated (`scan/eq_1_of/<n>`) | 11.4 µs | 47.8 µs |
| `iii-filter`, `FilterIndex::matches` (`index/eq_1_of/<n>`) | 133 ns | 211 ns |

A call with one matching observer costs the host 3.21 µs
(`call_local/1_observer_matching`), so the scan cost about 20 ns per observer
per call: at 4,000 observers, about 12,000 calls a second per host core.

### Engine benches

| Row | observe-v2 (`edc636d13`) | decision 55 | Δ | Rounds (observe-v2 / decision 55) | Verdict |
|---|---:|---:|---:|---|---|
| `observe_dispatch/default/0` | 1.83 µs | 1.85 µs | +0.9% | 1.69 µs, 1.97 µs / 1.87 µs, 1.82 µs | same within noise |
| `observe_dispatch/default/1` | 6.23 µs | 5.92 µs | -5.0% | 5.82 µs, 6.65 µs / 5.86 µs, 5.97 µs | same within noise |
| `observe_dispatch/default/8` | 11.9 µs | 11.7 µs | -1.8% | 12.2 µs, 11.7 µs / 12.8 µs, 10.6 µs | same within noise |
| `observe_dispatch/default/64` | 89.6 µs | 77.1 µs | -14.0% | 89.6 µs, 89.7 µs / 77.7 µs, 76.5 µs | faster |
| `observe_dispatch/filtered/16` | 6.95 µs | 5.25 µs | -24.4% | 6.02 µs, 7.87 µs / 5.52 µs, 4.98 µs | faster |
| `observe_dispatch/filtered/256` | 6.15 µs | 5.28 µs | -14.1% | 6.16 µs, 6.13 µs / 5.55 µs, 5.02 µs | faster |
| `observe_dispatch/filtered/1024` | 21.8 µs | 5.61 µs | -74.3% | 22.1 µs, 21.5 µs / 6.23 µs, 5 µs | faster |
| `observe_dispatch/filtered/4000` | 74.8 µs | 6.04 µs | -91.9% | 76 µs, 73.6 µs / 7.02 µs, 5.05 µs | faster |
| `observe_routed/filtered/16` | (no row) | 37.8 µs | | 37.1 µs, 38.5 µs | |
| `observe_routed/filtered/256` | (no row) | 38.2 µs | | 37.2 µs, 39.3 µs | |
| `observe_routed/filtered/1024` | (no row) | 37.7 µs | | 37.1 µs, 38.3 µs | |
| `observe_routed/filtered/4000` | (no row) | 36.5 µs | | 35.8 µs, 37.2 µs | |
| `observe_routed/matched/1` | (no row) | 41.8 µs | | 40.7 µs, 42.8 µs | |
| `observe_routed/matched/8` | (no row) | 50.8 µs | | 49.2 µs, 52.3 µs | |
| `observe_routed/matched/64` | (no row) | 108 µs | | 106 µs, 109 µs | |
| `observe_routed/observed/0` | 36.8 µs | 36.5 µs | -0.8% | 37.9 µs, 35.7 µs / 35.8 µs, 37.2 µs | same within noise |
| `observe_routed/observed/1` | 44 µs | 42.6 µs | -3.2% | 47.1 µs, 40.9 µs / 39.2 µs, 46.1 µs | same within noise |
| `observe_routed/observed/8` | 53.5 µs | 49.1 µs | -8.2% | 52.5 µs, 54.5 µs / 51.9 µs, 46.4 µs | faster |
| `observe_routed/observed/64` | 130 µs | 116 µs | -11.0% | 123 µs, 137 µs / 108 µs, 123 µs | same within noise |
| `control_plane_churn/observers_add_remove/100` | 405 µs | 413 µs | +1.8% | 400 µs, 406 µs, 418 µs, 397 µs / 402 µs, 408 µs, 423 µs, 418 µs | same within noise |
| `control_plane_churn/observers_add_remove/1000` | 4.62 ms | 4.55 ms | -1.4% | 4.94 ms, 4.43 ms, 4.7 ms, 4.4 ms / 4.54 ms, 4.53 ms, 4.49 ms, 4.65 ms | same within noise |
| `control_plane_churn/observers_add_remove/5000` | 25 ms | 26 ms | +4.0% | 24.5 ms, 24.8 ms, 26.2 ms, 24.6 ms / 27.4 ms, 25.4 ms, 25.4 ms, 26.1 ms | same within noise |
| `worker_cleanup/observers/10` | 54.3 µs | 56.3 µs | +3.8% | 57.6 µs, 50.9 µs / 59.5 µs, 53.2 µs | same within noise |
| `worker_cleanup/observers/50` | 191 µs | 195 µs | +2.1% | 202 µs, 179 µs / 204 µs, 185 µs | same within noise |
| `worker_cleanup/observers/200` | 787 µs | 800 µs | +1.7% | 821 µs, 753 µs / 837 µs, 763 µs | same within noise |
| `observe_fanout/1` | 14.4 µs | 14.4 µs | +0.5% | 16.6 µs, 13.3 µs, 13.4 µs, 14.1 µs / 17.3 µs, 13.3 µs, 13.6 µs, 13.5 µs | same within noise |
| `observe_fanout/8` | 35.2 µs | 34.8 µs | -1.1% | 40.1 µs, 31.7 µs, 32.4 µs, 36.5 µs / 44.1 µs, 30.8 µs, 31.5 µs, 32.6 µs | same within noise |
| `observe_fanout/32` | 68.6 µs | 77.5 µs | +12.9% | 73.8 µs, 66.7 µs, 65.9 µs, 68.2 µs / 101 µs, 74.5 µs, 66.5 µs, 67.7 µs | same within noise |
| `observe_fanout/128` | 182 µs | 201 µs | +10.5% | 191 µs, 177 µs, 183 µs, 177 µs / 244 µs, 202 µs, 181 µs, 177 µs | same within noise |

`observe_routed/<case>/<n>` rows have no observe-v2 row of their name.
`observed` answers with the result alone (no match hook), and the engine
evaluates the `n` unfiltered observers; observe-v2's `observed` rows measured
a host that listed every observer, the wire `matched` measures. `filtered` is
the load test's shape, `n` observers each on its own `input.k`, one
matching: 37.8 µs a call with 16 observers and 36.5 µs with 4,000, the cost
of an unobserved routed call (`observed/0`, 36.5 µs) plus one activation.
Every case took one `invokefunction` and one `invocationresult` a call and no
other frame (6,096,502 calls over the two rounds).

`observe_dispatch/filtered/<n>` costs 5.25 µs with 16 observers and 6.04 µs
with 4,000. A function none of whose observers has an indexable filter keeps
no index, so `observe_dispatch/default`, `observe_fanout`,
`control_plane_churn/observers_add_remove` and `worker_cleanup/observers`
(unfiltered observers) run the code of observe-v2: what separates their
rounds (`observe_dispatch/default/64` 14% faster in both, one round of
`observe_fanout/32` at 101 µs) is the machine's load, and the other rows of
unfiltered observers are the same within noise. Kept for every function,
the index cost
`control_plane_churn/observers_add_remove` 16%, 24% and 26% with 100, 1,000
and 5,000 observers in the same conditions.

### Load test

Not run. The `filters` scenario's host is the Rust SDK, which evaluates the
filters of its routed calls until it answers with decision 55's `matched` and
`delivered`; the scenario measures decision 55 with the SDKs that do.

## Slower than 0.25

With the observer churn fix (`d37f78f42`), from its pass; first-pass figures
where the fix's pass did not measure the row.

- **`observe_fanout/1`**: 13.1 µs against 11.4 µs for `trigger_fanout/1`
  (+14.9%; +14.5% in the first pass). **`observe_fanout/8`**: 30.6 µs against
  28.7 µs (+6.4%), where observe-v2 before the fix measured +5.5% in the same
  rounds and +4.8% (the same within noise) in the first pass. With 32 and 128,
  0.26 takes 21% and 45% less time.
- **`observe_routed/observed/8` and `/64`**: 47.4 µs and 101 µs against 44.2
  µs and 93.4 µs (+7.3%, +8.1%; +6.9% and +9.4% in the first pass). In 0.26
  the host's `invocationresult` lists the matched observers (8 or 64 ids) and
  the engine starts their activations from that list; in 0.25 the engine
  evaluated the observers itself. Unobserved and with one observer, the two
  are the same within noise.
- **Adding and removing observers of one function.**
  `control_plane_churn/observers_add_remove/<n>` adds and removes `n` engine
  observers of one function: one add and remove costs 3.97 µs with 100
  observers, 4.34 µs with 1,000 and 4.92 µs with 5,000. 0.25's closest bench
  registers and unregisters `n` triggers of `n` functions at 1.38 to 1.50 µs
  each, so 0.26 costs 2.9x to 3.3x. With the shape of 0.25's bench (one source,
  `n` targets) 0.26 costs 3.2 to 3.6 µs a change, 2.3x to 2.4x. Before the fix
  one change cost 6.26, 21.5 and 90.9 µs, and the total grew with the square
  of `n`.
- **Cleaning up a worker's observers.** `worker_cleanup/observers/<n>` costs
  3.5 to 5.0 µs per observer (10 to 200 observers), against 0.86 to 1.86 µs
  per trigger for 0.25's `worker_cleanup/triggers/<n>`: 2.7x to 4.0x (4.2 to
  5.7 µs per observer before the fix).
- **Load test `filters`.** With 1,024 and 4,000 filtered observers of one
  function, 0.26 takes 30.9k and 4.6k calls/s against 77.9k and 28.3k (−60%,
  −84%), with the engine's CPUs mostly idle (1.4 and 0.4 of 4 cores busy). It
  delivers every matching call (0 dropped); 0.25 delivered 33.7k and 5.0k
  activations/s and dropped 386M and 670M observations as `OVERFLOW`. Counted
  in activations delivered, 0.26 is 8.3% and 8.0% below 0.25. With 256
  observers 0.26 is 10.0% below and with 16, 5.3% (7.8% and 3.7% in the first
  pass). The fix's calls and activations are the same as before the fix
  within noise.
- **Load test `fanout`.** With 4, 16 and 64 observers a function, 0.26 takes
  17%, 14% and 31% fewer calls per second (four runs; 13%, 16% and 36% in the
  first pass, before the fix). Activations delivered are the same within noise
  with 1, 4, 16 and 64 observers. With 64 observers the fix takes 21% more
  calls per second and delivers 21% more activations than observe-v2 before
  it.
- **Load test `observers`.** Engine memory per observer at rest is 9.1 KiB
  against 7.8 KiB with 4,000 observers (+16.8%), 9.1 against 8.1 KiB with
  16,000 (+11.9%) and 9.7 against 7.8 KiB with 64,000 (+24.0%). Before the fix
  the runs of the same pass measured 8.2, 8.1 and 8.9 KiB, so the fix holds
  8.5% to 12.8% more. With 1,000 observers, runs of every version measure about 7.1 or about
  9.7 KiB, and the fix's two measured 9.6 and 9.7. Creating and disposing of
  observers is faster than in 0.25: 64,000 created at 16.3k a second and
  disposed of at 10.5k, against 4.8k and 4.6k.

## Check

A second pass, on observe-v2 at `e0d87164c`, re-ran every row the first pass
marked a regression or over budget, plus the two largest improvements
(`observe_fanout/128` and the `source` load test at 200k activations/s), at
least twice each, alternating which version ran first. The committed numbers
match the raw data. What the second pass found:

- Every regression of the first pass holds, at the same size.
- The two improvements hold.
- `control_plane_churn/observers_add_remove`: a matched bench with one source
  and n targets, using the same observe and unobserve calls in both versions,
  gives 2.9x, 5.3x and 16.5x the 0.25 cost at 100, 1,000 and 5,000 on
  observe-v2 before the observer churn fix, and 2.3x, 2.3x and 2.4x with the
  fix ("Observer churn fix").
- §13, 10 observers none matching, at 5,000 calls/s: the p99 is 7.4 to 9.1 µs
  in all five rounds, above the 5 µs budget; mean and p90 are under it. A call
  with no observers has a p99 above 5 µs too, so the tail comes from the paced
  loop on this machine, and the budget's 5 µs reads as a mean.
- The `source` load test makes `rate / 16` calls a second, each reaching 16
  observers with the event carried once; 0.25's `emit` makes one emit per
  activation. The two compare activations delivered, not calls.
- Machine: one unrelated process at 100% of a core throughout, Spotlight at
  about 90% during the load tests, load average 5 to 9.5, no thermal warning.

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

# Load test (Linux; here in Docker, linux/arm64)
cargo build --release -p iii -p iii-loadtest
target/release/iii-loadtest suite --profiles host --engine-cpus 4-7 --load-cpus 0-3 \
  --scenarios rpc,fanout,filters,observers,source,match-keys --port 58010
```
