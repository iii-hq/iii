# Consistent invocation snapshots (source-only)

## Provenance and scope

Base: `16e95042639bab6f7f81fadfdd410233d2044f2b`, branch
`fix/consistent-invocation-snapshot`, worktree
`/home/doggao/iii/wt/iii.metrics-consistent-snapshot`.
The owner verified ancestry from `c5129b8da5c0f4db8719ae6dd58aee6fa297bc9b`
and byte-identical focal sources before dispatch. This change retains the
Registry root-binding fix17 by building on that base; it does not integrate
another branch, replace an engine, or change protected/smoke-owner sources.

Run15's rejected BEFORE pairs were 304/305 and 330/331. The exact interleaving
remains unknown. This is not evidence of a lost increment, and current-call
selfmetrics do not explain the extra map count. The six failed own15 records
and smoke equality/limits/oracles remain immutable. No runtime acceptance is
claimed here.

## Contract

`MetricsAccumulator` privately owns one `Mutex<InvocationState>` containing
four `u64` aggregates and `HashMap<String, u64>`. Every dispatcher outcome and
each owner-validated `LocalFunctionStats` row updates total, outcome buckets
and its function count in one critical section. A whole SDK message is **not**
one transaction. Errors remain clamped to count; count-zero rows still insert
a zero entry. No total recomputation, clipping repair, key filtering, or minus
one adjustment is used.

`snapshot_invocations` copies all four numbers and the entire map under that
same mutex into the existing `InvocationsView`. This is a common cut of the
invocation group only. Workers, performance, SDK metric storage, timestamp,
OTEL instruments and the complete API response are not made atomic with it.
`invocation_counts` copies four numbers without allocating/cloning a map for
alerts. Single-function and map getters return owned values; the iterator owns
its map, so consuming or dropping it never keeps a lock. Separate getter calls
are not one common cut.

Success is counted after handler completion and macro serialization. NoResult
uses success without setting the first-user-success marker. Failure retains
its existing first-user-failure behavior. Deferred is counted before
`receiver.await`, without counting the later completion again. The current
metrics query is excluded from its already-serialized response; the next query
includes the preceding completed one. The existing first-user OnceLocks and
worker counters are unchanged.

## Locking, unwind and overflow

All guards are private and lexical. No await, external lock acquisition,
ownership lookup, OTEL/storage/performance call, serialization, logging or
user callback occurs while the invocation mutex is held. The map's String key
hashing invokes no caller code. Existing-key updates avoid a String allocation;
a missing entry is allocated before numeric mutation. The commit then consists
only of explicit nonpanicking numeric operations. Snapshot cloning reads but
does not mutate state. Allocation failure that aborts the process is not a
recoverable transaction.

Poison recovery takes the existing guard, never resets or repairs a partial
state. Its justification is that production has no unwinding operation between
numeric writes. This invariant must be preserved by future edits. Tests poison
only complete states before/after a whole commit; they never sanction partial
recovery. Instance-local `cfg(test)` barriers pause actual commit/copy stages;
there is no production callback hook. A contending acquisition acknowledges
`try_lock == WouldBlock` in the same acquisition method, then uses the mutex.
The barrier protocol is intentionally not a timing assertion; broken protocols
can hang and should be bounded by the future test runner's external watchdog.

Dispatcher aggregates retain `wrapping_add` (previously AtomicU64 fetch_add).
SDK aggregates and map entries independently retain `saturating_add`. A common
cut is still provided after overflow, but equality of sums and monotonicity
require a history without overflow or saturation. Mixed-policy/MAX tests make
this distinction explicit; this patch does not unify policies silently.

## Privacy, compatibility and cost

JSON schema, metric names, cumulative/global scope and namespace behavior are
unchanged. Metrics are not caller-scoped. `observable: false` still counts;
privacy refusals occur before counting, and no new input/session/caller/env
capture is introduced. The registration macro is unchanged.

Removing public atomic/map fields and map-only mutators is an intentional Rust
source API break. Consumers must use whole transactions and owned readers.
`iter_function_counts` now allocates an owned map; it is no longer a borrowed
DashMap iterator. No unrelated DashMap or accumulator was converted, and no
dependency/toolchain/manifest change is needed. The benchmark source now measures
a whole transaction instead of a map-only update; old measurements are not
comparable. A global mutex serializes hot-path writes and map clones are O(keys),
blocking writers and potentially Tokio threads. No performance result or bound
is asserted without measurements.

## Source inventory

- `engine/src/workers/observability/metrics.rs`: storage, transaction/getter APIs,
  counts-only AlertManager reader; existing accumulator tests and coherent
  test-only resets for alert fixtures. Worker atomics remain separate.
- `engine/src/invocation/mod.rs`: Success/Failure/NoResult/Deferred writers;
  cfg(test)-only per-handler accumulator override, no global-reset race in new tests.
- `engine/src/observer/dispatch.rs`: owner validation/OTEL remain outside lock;
  validated rows use the transaction. Private accumulator-parameterized body
  permits isolated tests. Obsolete atomic saturation helper removed; its test
  now exercises the actual accumulator.
- `engine/src/workers/observability/mod.rs`: one owned snapshot before performance
  and serialization. Private implementation accepts an accumulator for isolated
  tests; only the existing list_metrics function is registered in production.
- `engine/src/workers/telemetry/mod.rs`: test-only reset/fixture adaptation.
  Commented-out old aggregate readers are inactive; live first-user readers stay.
- `engine/benches/core_runtime_bench.rs`: adapt map-only calls to whole updates.
- Four `snapshot_tests.rs` modules under invocation, observer/dispatch,
  observability and observability/metrics contain the new tests.
- Existing `engine/tests/observe_owner_e2e.rs` owner/saturation/private tests use
  preserved getters and need no edits. Existing `observe_routed_e2e.rs` remains
  unchanged. New isolated unit tests cover these semantics without a full replay.

## Authored tests, not executed

New tests cover actual partial-writer/reader and reverse partial-reader/writer
schedules, exact tuple+map before/after states, first insertion/existing key,
SDK rows and a deterministic reproduction of the legacy split-read schedule.
The existing concurrent-first-insert test now asserts the whole snapshot.
Additional tests cover owned iterator/reentrant reads, wrapping/saturation,
poison with intact state, outcomes and first-user markers, manually polled
Deferred Pending, owner/namespace refusals, unobservable/no-extra-capture,
and selfmetrics via the real service macro, list implementation and dispatcher.
No new test relies on sleep or race probability. New accumulators are isolated;
legacy serial global fixtures remain serial and reset only complete groups.

The following are **proposed only**, require separate test/build authorization,
and were NOT run. From this worktree, select narrowly (no full-engine replay):

```sh
cargo test --manifest-path engine/Cargo.toml --lib snapshot_tests::
cargo test --manifest-path engine/Cargo.toml --lib test_accumulator_
cargo test --manifest-path engine/Cargo.toml --lib test_metrics_accumulator_
cargo test --manifest-path engine/Cargo.toml --lib test_first_user_
cargo test --manifest-path engine/Cargo.toml --lib sdk_drop_reasons_are_bucketed_and_counts_saturate
cargo test --manifest-path engine/Cargo.toml --lib alert_manager_fires_then_prunes_state_on_rule_removal
cargo test --manifest-path engine/Cargo.toml --lib alert_manager_resurrection_filter_hides_removed_rule
cargo test --manifest-path engine/Cargo.toml --lib test_telemetry_module_background_tasks_and_destroy_run_without_network
cargo test --manifest-path engine/Cargo.toml --test observe_owner_e2e local_invocation_stats_count_the_owners_functions_only -- --exact
cargo test --manifest-path engine/Cargo.toml --test observe_owner_e2e local_invocation_counts_saturate -- --exact
```

Source-only checks: direct existing rustfmt (edition 2024, skip_children=true,
explicit edited files), `git diff --check`, scoped exact-symbol inventory and
manual diff review. Formatting parsing does not type-check, expand macros or
prove tests pass. No Cargo/rustc/clippy/benchmark/build/runtime/network/ELF
operation is authorized or performed. Rust supplementary reference chapters
were inaccessible under the filesystem boundary; the loaded Rust skill was
used without bypassing that restriction. Review this commit before separately
authorizing tests, build/retention or fresh own15e2 adoption.
