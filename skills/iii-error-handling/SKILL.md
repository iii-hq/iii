---
name: iii-error-handling
description: >-
  Handle iii engine and SDK errors across Node, Python, Rust, and browser
  workers. Use when interpreting error codes, retryability, RBAC denial,
  timeouts, handler failures, or SDK-specific exception surfaces.
---

# Error Handling

iii has two broad error classes: SDK/local errors and engine/remote invocation errors. Agents should branch on the error code instead of matching only message strings.

## Error Codes

Branch on exact `code` strings, but keep engine wire codes separate from SDK-local codes.

| Code | Emitted by | Meaning | Typical handling |
| --- | --- | --- | --- |
| `function_not_found` | Engine and SDK local dispatch | No registered function is available under that ID | Check function ID, worker install/startup, and discovery (`engine::functions::list`) |
| `invocation_error` | Engine invocation/router path | Engine failed to route, record, or complete the invocation | Inspect engine logs, protocol state, and worker connectivity |
| `invocation_stopped` | Engine invocation handler | Invocation was cancelled or stopped by the engine/runtime | Treat as failed work; decide whether caller should retry |
| `FORBIDDEN` | RBAC / worker-gated engine functions | RBAC denied the action | Do not retry blindly; inspect policy, auth context, and allowed functions |
| `timeout` | A target worker's handler, forwarded verbatim by the engine | Not produced by the engine or the Node/Python SDKs (they emit `TIMEOUT`); appears only if the worker you called returns it | Treat as a timeout if you know the target worker emits it; otherwise branch on `TIMEOUT` |
| `function_not_invokable` | SDK local dispatch | Registration exists but cannot be invoked as a normal local function | Inspect registration/invocation type |
| `invocation_failed` | SDK worker handler wrappers | Local worker handler, HTTP-invoked function wrapper, or SDK-side handler path failed | Inspect handler logs, stacktrace, and payload validation |
| `TIMEOUT` | Node/Python SDK caller timeout | Client waited longer than the `call()` timeout | Increase timeout only if the workload is expected to run long; otherwise optimize or enqueue |
| `payload_too_large` | SDK | An outgoing message exceeds 16 MiB | Use a channel; a call to a function in the same worker runs in-process (no limit) unless sent with `dispatch: 'remote'` |

## Observer Errors

`iii.observe()` never throws for engine-side problems: the handle turns `error` and `ready()`
rejects with an `ObserveRejection` (Go `*iii.ObserveRejectionError`) carrying the code.

| Code | Meaning | Typical handling |
| --- | --- | --- |
| `UNSUPPORTED` | The engine is older than 0.25, or `on` / `capture` / `deliver` has an unknown value | Upgrade the engine or fix the source |
| `INVALID_ARGS` | `args` fail the function's `args_format`, or args were sent to a function without one | Fix the args; check the function's `args_format` in `engine::functions::list` |
| `INVALID_FILTER` / `INVALID_FILTER_PATH` | The filter is malformed, over a limit, uses literal shorthand, or names a path the function's schema does not have | Use explicit operators (`{ "$eq": v }`); check the path against `request_format` |
| `NOT_OBSERVABLE` | The function registered `observable: false` | Do not retry |
| `FORBIDDEN` | RBAC denies the observed function or the target, or the registration hook denied | Fix the policy, not the call |
| `SELF_TARGET` | The target is the observed function | Use another target |
| `LIMIT_EXCEEDED` | `observers.max_per_connection` / `max_per_function` reached | Dispose unused observers or raise the limit |
| `OBSERVER_ID_CONFLICT` | Another observer holds the `id`: the same connection's, with a different spec, or another connection's. A `suspended` observer (its observing worker away) keeps its `id` from every other worker until the grace period (`observers.observer_grace_ms`, 30 s) ends | Use a new id. A worker back on a new connection, under the same worker name in the same namespace (and the same principal: both on ports without `rbac`, or both on RBAC ports with the same non-empty auth `context`), takes over its own `suspended` observer's `id`: its `observe` replaces that observer, so a restarted worker keeps its observers (and a queue consumer its backlog) under stable ids. The name must stay the same across restarts (`III_WORKER_NAME`); under the default `hostname:pid` a restarted worker is another worker |
| `OWNER_TIMEOUT` / `OBSERVE_REJECTED` / worker codes | The hosting worker's `onObserverAdded` timed out, threw, or rejected with its own code (`HTTP_ROUTE_CONFLICT`, `INVALID_CRON_EXPRESSION`, ...) | Read `reason.message`; fix the args |

A worker that calls a target through one of its function's observers
(`iii.call({ observer: obs, payload })`) gets `NOT_FUNCTION_OWNER` (it does not host the observed
function), `OBSERVER_NOT_FOUND`, `OBSERVER_NOT_ACTIVE`, `OVERFLOW` (over `max_in_flight`),
`LOOP_GUARD`, `TIMEOUT`, or the target's own error. A call to another worker's `private` function
fails with `FORBIDDEN`. iii's own deliveries are fire-and-forget: their failures and drops show up
in `iii.observer.dropped.total` / `iii.observer.activation.failures.total` and in
`engine::observers::get`, never in the observed call. See `iii-observe`.

## Handler vs Engine Errors

- Handler errors originate in user function code, SDK local dispatch, or HTTP-invoked endpoints.
- Engine errors originate in routing, invocation state, RBAC, protocol handling, or worker-reported wire errors.
- Queue retries only apply to enqueued work. Synchronous failures are returned directly to the caller.
- Void dispatch does not return handler results, so use logs/observability for failures.

## Retryability

- Retry transient `TIMEOUT` (or a lowercase `timeout` returned by a target worker), transport, or worker reconnect failures only when the operation is idempotent.
- Do not retry `FORBIDDEN` without changing auth/policy.
- Do not retry `function_not_found` by calling the same ID repeatedly; discover functions or install/start the missing worker.
- For reliable background work, use `CallAction.Enqueue({ queue })` and queue retry/DLQ policy.

## SDK Surfaces

### Node

```typescript
import { InvocationError } from 'iii-sdk'

try {
  await iii.call({ function_id: 'orders::charge', payload })
} catch (error) {
  if (error instanceof InvocationError && error.code === 'FORBIDDEN') {
    throw new Error('Policy denied orders::charge')
  }
  throw error
}
```

### Python

```python
from iii import InvocationError

try:
    result = iii.call({"function_id": "orders::charge", "payload": payload})
except InvocationError as exc:
    if exc.code == "FORBIDDEN":
        raise RuntimeError("Policy denied orders::charge")
    if exc.code in ("TIMEOUT", "timeout"):  # SDK caller timeout, or a lowercase code returned by the target worker
        raise RuntimeError("orders::charge timed out")
    raise RuntimeError(f"{exc.code}: {exc.message}")
```

### Rust

```rust
match iii.call(request).await {
    Ok(value) => value,
    Err(iii_sdk::Error::Timeout) => {
        return Err("orders::charge timed out".into());
    }
    Err(iii_sdk::Error::Remote { code, message, .. }) if code == "FORBIDDEN" => {
        return Err(format!("policy denied: {message}").into());
    }
    Err(err) => return Err(err.into()),
}
```

### Browser

Browser calls (`iii.call`) reject with JavaScript errors. Preserve the engine-provided code/message when present and show policy failures as permission errors in UI.

## Pattern Boundaries

- For invocation modes and enqueue decisions, prefer `iii-core-primitives`.
- For observer states, filters, and observer hooks, prefer `iii-observe`.
- For SDK-specific exception classes and syntax, prefer `iii-sdk-reference`.
- For workflow-level retry and DLQ design, prefer `iii-architecture-patterns`.
- For RBAC policy design see the [rbac-proxy worker](https://workers.iii.dev/workers/rbac-proxy); for logs and traces around worker failures, use the matching worker page on https://workers.iii.dev/.

## When to Use

- Use this skill when the task mentions iii errors, exception handling, failed invocations, timeouts, forbidden calls, retry behavior, or SDK error classes.

## Boundaries

- Do not retry non-idempotent work automatically unless it is enqueued under queue policy.
- Do not treat RBAC denial as a missing worker.
- Do not generate removed service APIs or adapter-extension APIs.
