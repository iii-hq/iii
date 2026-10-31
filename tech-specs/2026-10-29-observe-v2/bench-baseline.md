# Observe v2 benchmark baseline

What observe v2 (0.26.0) costs, measured against the performance budget of the
spec (§13) and against 0.25.0 on the same machine. Every number here is in
`bench-baseline.json`, run by run, with the machine, the toolchain and the
commits.

## Summary

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
- Engine benches against 0.25: 65 rows the same within noise, 20 faster, 3
  slower beyond noise (`observe_fanout/1`, `observe_routed/observed/8` and
  `/64`), and 6 slower than their closest 0.25 bench, which has another shape
  (adding, removing and cleaning up observers; see the end of this file).
- Load test against 0.25: `rpc` is the same; `source` delivers 200k
  activations/s sustained where 0.25's `emit` delivered 100k; `filters` and
  `fanout` take fewer calls per second from 256 and 4 observers on, and
  `filters` drops nothing where 0.25 dropped most of its observations.

## Setup

| | |
|---|---|
| Machine | Apple M4 Max, 16 cores (12 performance, 4 efficiency), 64 GB, macOS 15.7.4 (24G517) |
| Other load | one unrelated process at 100% of one core and a browser renderer at about 70% of one throughout; load average 6 to 10. Versions alternate run by run, and every run is repeated. |
| 0.25.0 | `b0b86fd37` (main; observe-v2 branched from it) |
| 0.26.0 | `e0d87164c` (observe-v2), with the bench output this file's commit adds: `observe_routed_bench` prints the frames per call, `observe_bench` runs each local call again at 5,000 calls/s. Neither changes the code measured. |
| Rust | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1, aarch64-apple-darwin; criterion 0.5.1; the `bench` profile is the release profile (lto, codegen-units 1, opt-level 2) |
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
| Event source (handler returns its input), observed | the event serialized once | `call_local/event_source_observed` 3.64 µs a call; at 5,000 calls/s 1.000 report items a call, 1.000 of them carrying the event once (`output_is_input`, no `output`). Node: every `bench::source` item carries it once. | no equivalent (0.25 sources emitted with `obs.emit()`) | within budget |
| Local call to a private function | no copy of input or result when nothing reads the input after the handler; one copy of the input when a hook, a filter or a `capture` reads it | `call_local/private` 1.91 µs (the handler alone: 51.2 ns); `call_local/private_event` 2.21 µs, the same with a deliver hook `call_local/private_deliver_hook` 2.92 µs. `a_local_call_of_a_private_function_copies_nothing` passes (allocations do not grow with the payload). | no equivalent (0.25 has no private functions) | within budget |
| Call through an observer | one routed invocation | No bench times it. `a_call_through_an_observer_is_the_fixture_frame` passes: the call is one `invokefunction` to the observer's target. | no equivalent | within budget (counted by the test) |

## Measurements

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
| `control_plane_churn/observers_add_remove/100` | 138 µs | 624 µs | +350.7% | 0.1% / 0.4% | 2 / 2 | slower than the closest 0.25 equivalent (different shape) | `control_plane_churn/triggers_register_unregister/100` (1) |
| `control_plane_churn/observers_add_remove/1000` | 1.42 ms | 21.5 ms | +1416.9% | 0.1% / 0.1% | 2 / 2 | slower than the closest 0.25 equivalent (different shape) | `control_plane_churn/triggers_register_unregister/1000` (1) |
| `control_plane_churn/observers_add_remove/5000` | 7.56 ms | 456 ms | +5933.9% | 0.1% / 0.2% | 2 / 2 | slower than the closest 0.25 equivalent (different shape) | `control_plane_churn/triggers_register_unregister/5000` (1) |
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
| `worker_cleanup/observers/10` | 18.7 µs | 56.6 µs | +203.6% | 0.3% / 1.6% | 2 / 2 | slower than the closest 0.25 equivalent (different shape) | `worker_cleanup/triggers/10` (2) |
| `worker_cleanup/observers/50` | 50.5 µs | 213 µs | +322.5% | 0.4% / 4.2% | 2 / 2 | slower than the closest 0.25 equivalent (different shape) | `worker_cleanup/triggers/50` (2) |
| `worker_cleanup/observers/200` | 174 µs | 1.16 ms | +570.9% | 0.3% / 0.1% | 2 / 2 | slower than the closest 0.25 equivalent (different shape) | `worker_cleanup/triggers/200` (2) |
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

(1) Different shape: 0.25 n triggers on n functions; 0.26 n engine observers on one function.  
(2) Different shape: 0.25 a worker's n triggers unregistered; 0.26 a worker's n observers of one function suspended.  

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
| 64 observers/function | 193k / 190k | 171k / 154k | -15.2% | regression vs 0.25 beyond noise | 41.5M / 41.6M → 27.4M / 25M |

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

## Slower than 0.25

- **`observe_fanout/1`**: 13.1 µs against 11.5 µs for `trigger_fanout/1`
  (+14.5%). With 8 observers the two are the same within noise; with 32 and
  128, 0.26 takes 22% and 45% less time.
- **`observe_routed/observed/8` and `/64`**: 48.3 µs and 99.2 µs against 45.2
  µs and 90.7 µs (+6.9%, +9.4%). In 0.26 the host's `invocationresult` lists
  the matched observers (8 or 64 ids) and the engine starts their activations
  from that list; in 0.25 the engine evaluated the observers itself.
  Unobserved and with one observer, the two are the same within noise.
- **Adding and removing observers of one function.**
  `control_plane_churn/observers_add_remove/<n>` adds and removes `n` engine
  observers of one function: one add and remove costs 6.2 µs with 100
  observers, 21.5 µs with 1,000 and 91 µs with 5,000, so the total grows with
  the square of `n`. 0.25's closest bench registers and unregisters `n`
  triggers of `n` functions at 1.4 to 1.5 µs each. The load test shows the
  same cost: the `match-keys` worker creates its observers of one function at
  35,500 a second up to 100, 16,000 to 17,800 a second up to 1,000 and 1,500 a
  second up to 10,000.
- **Cleaning up a worker's observers.** `worker_cleanup/observers/<n>` costs
  4.3 to 5.8 µs per observer (10 to 200 observers), against 0.9 to 1.9 µs per
  trigger for 0.25's `worker_cleanup/triggers/<n>`.
- **Load test `filters`.** With 1,024 and 4,000 filtered observers of one
  function, 0.26 takes 30.5k and 4.7k calls/s against 78k and 28k (−61%,
  −83%), with the engine's CPUs mostly idle (1.4 and 0.4 of 4 cores busy). It
  delivers every matching call (0 dropped); 0.25 delivered 33.6k and 5.0k
  activations/s and dropped 389M and 672M observations as `OVERFLOW`. Counted
  in activations delivered, 0.26 is 9.3% and 6.5% below 0.25. With 256
  observers 0.26 is 7.8% below; with 16 the two are the same within noise.
- **Load test `fanout`.** With 4, 16 and 64 observers a function, 0.26 takes
  13%, 16% and 36% fewer calls per second. Activations delivered are 5% higher
  with 1 observer, the same within noise with 4 and 16, and 15% lower with 64.
- **Load test `observers`.** Engine memory per observer at rest is 8.3 KiB
  against 7.3 KiB with 4,000 observers (+13.9%) and 8.8 KiB against 7.8 KiB
  with 64,000 (+12.3%); with 1,000 and 16,000 the two are the same within
  noise. Creating the observers up to 64,000 and disposing of them run 14% and
  9% slower; creating the first 1,000 and 4,000 runs 83% and 44% faster.

## Reproducing

```sh
# Engine benches (each version in its own checkout and target directory)
cargo bench -p iii --bench observe_dispatch_bench -- --save-baseline r1 --noplot
cargo bench -p iii --bench observe_fanout_bench -- --save-baseline r1 --noplot   # 0.25: trigger_fanout_bench
cargo bench -p iii --bench observe_routed_bench -- --save-baseline r1 --noplot

# Rust SDK (criterion rows, then the paced pass at 5,000 calls/s)
cargo bench -p iii-sdk --bench observe_bench -- --save-baseline r1 --noplot

# Node SDK
pnpm --filter iii-sdk bench

# Go SDK
(cd sdk/packages/go/iii && go test -run '^$' -bench '^BenchmarkIsObserved$' -benchmem -count 10 .)

# Load test (Linux; here in Docker, linux/arm64)
cargo build --release -p iii -p iii-loadtest
target/release/iii-loadtest suite --profiles host --engine-cpus 4-7 --load-cpus 0-3 \
  --scenarios rpc,fanout,filters,observers,source,match-keys --port 58010
```
