# Current Compose status snapshots

`compose::status` reads the loaded project's current in-memory state, supervised
children and declared configuration together. It waits for the lifecycle mutex
**before** taking the declaration read lock (`inner -> file`), matching up and
reconcile. Lock contention is not evidence that a container stopped or that the
daemon lost ownership. The persisted state store is never a status fallback.

After both guards are acquired, status performs only synchronous child polls and
copies owned response rows. It releases both guards before JSON serialization or
network work. This is a coherent project snapshot, not a new liveness protocol:
existing running/unknown/reaped polling and record-to-state rules are unchanged.
A child may exit after being polled, just as before.

## Waiting and errors

- Long startup can delay status until the lifecycle mutation releases `inner`.
  This exposes committed in-memory state; it does not wait for the later durable
  state save. Health/registration by itself is not a lifecycle commit barrier.
- Both lock acquisitions observe the existing project/daemon shutdown signal.
  Cancellation returns the existing `OPERATION_CANCELLED` error, not rows.
  An already-cancelled signal wins even if both locks are available. A genuine
  stopped project queried before shutdown cancellation still returns stopped.
- The remote status handler bounds snapshot acquisition with the existing
  published 10,000 ms read budget. Expiry drops the waiting future and returns
  the SDK's existing `TIMEOUT` error. It does not cancel the lifecycle operation.
  Initial project loading/recovery keeps its existing lifecycle behavior and is
  outside this new server-side acquisition timer; the caller's RPC deadline
  still bounds its own request. No new cancellation point interrupts adoption
  or cleanup during recovery.
- `compose::list` uses the same status method for each loaded project, sequentially.
  A busy project can delay later entries (head-of-line blocking). The existing
  10,000 ms read budget bounds the entire remote list acquisition, not each row.
  Any acquisition error fails the whole list, never a successful partial/empty
  substitute. Membership still skips cells not yet loaded. The list is not an
  atomic snapshot across projects.
- Rust callers can drop or wrap either read future with their own deadline.
  The direct Rust status method is shutdown-cancellable, not independently timed.

## Compatibility and preserved semantics

Successful JSON and its schemas are unchanged: `container`, `state`, `version`,
`requested`, `pid`, `owned`, `log_path`, and `last_error` retain their existing
serialization and omission rules. No `busy`, provenance, or synthesized-ready
field is introduced. Remote failures use the existing error envelope and codes.

Rust source compatibility changes: `Project::status` now returns
`Result<Vec<ContainerStatus>>`, and `Daemon::list` returns
`Result<Vec<serde_json::Value>>`. In-tree callers propagate the error or explicitly
unwrap in tests. `Daemon::status` still resolves a project and retains its type.
CLI JSON consumers and reporting need no success-shape change.

Quiescent projects with no record/child remain stopped and unowned. A persisted
ready record cannot override current memory. A current ready record with no child
retains the previous ready/unowned/no-PID behavior; this patch does not redesign
record semantics. Reaped children retain ownership until lifecycle removes them,
without a running PID. Process birth-identity and foreign-project binding guards
are untouched, as are Registry, metrics and smoke oracles.

## Source-only regression coverage

`project::status_tests` uses a private `ProjectFixture`, an unstarted SDK client,
and test-only watch-backed supervised handles on Unix and Windows. It invokes the
real status method, not a mock projection. The held inner guard is the lifecycle
commit barrier: the child exists before the status future is explicitly polled
pending, then the ready record is committed and the guard released. Both missing
and stopped persisted state are covered without saving the new record first.
With the old acquisition body (wrapped in `Ok` only for the candidate signature),
the same pending assertion would fail by returning stopped/unowned immediately.
This is a source counterexample, not an executed before/after result.

Other cases prove reconcile can acquire `file.write()` while status waits for
`inner`, cancellation at either acquisition, pre-cancellation, dropped-request
lock release, genuine stopped output, stale persisted ready isolation, and
reaped-child PID behavior. Pending/ready polls and `try_write` are deterministic
oracles; there are no sleeps, external processes, VM adapters or engine connections.
Existing remote status/list schema and equality tests are retained unchanged.

Tests are **AUTHORED_NOTRUN** in this source-only change. A later authorized review
may run the focused `project::status_tests` library tests with `--locked --offline
-j4 -p iii-compose --lib`, then remote schema/conformance and affected caller tests.
No compilation, runtime or fresh smoke success is claimed here. The precise
runtime branch behind smoke 15's earlier stopped/unowned response remains unknown.
