---
type: how-to
functions: [bridge.invoke, bridge.invoke_async]
title: Ad-hoc invoke a function on the remote iii engine
---

# When to use

**First, check whether you can use a `forward:` alias instead.** The recommended way to call across the bridge is to configure a `forward:` entry on the `iii-bridge` worker (`{ local_function: "my::feature::do-thing", remote_function: "actual::remote::id", timeout_ms? }`) and then call `local_function` like any other engine function. The worker dynamically registers `local_function`, so the bridge is invisible at the call site and the function id is stable across the codebase. **`bridge.invoke` and `bridge.invoke_async` are ad-hoc escape hatches** for the cases where a forward alias is wrong or impossible — they should not be the default reach.

Reach for these escape hatches only when:

- The remote `function_id` is **dynamic at runtime** (looked up from a registry, computed from user input, etc.) — a `forward:` alias requires a static `remote_function` in config.
- You're **prototyping** or running a one-off operator script and don't want to edit `iii-config.yaml` for a single call.
- You need to **probe** the remote engine to verify connectivity or list available functions before deciding whether to wire a `forward:` alias.

Once the choice to use the escape hatch is made, the two functions differ only in whether the call round-trips:

| Question                                                              | Use this              |
|-----------------------------------------------------------------------|-----------------------|
| Need the remote function's return value to continue?                  | `bridge.invoke`       |
| Want to forward telemetry, logs, or events without blocking?          | `bridge.invoke_async` |
| Need timeout protection (slow remote should not hold up the caller)?  | `bridge.invoke` with `timeout_ms` set |
| Will you call the same `(local, remote)` pair many times?             | **Don't use either** — configure a `forward:` alias and call `local_function` directly |

# `bridge.invoke`

## Inputs

```json
{
  "function_id": "engine::log::info",                  // required. Remote function id to invoke. Must match a function the remote engine has registered.
  "data":        { "message": "hello from local" },    // optional. JSON payload forwarded to the remote function. Defaults to null when omitted.
  "timeout_ms":  5000                                  // optional. Per-call timeout in milliseconds. Defaults to 30000 (30 seconds).
}
```

`function_id` is required. Empty `function_id` or any payload that fails to deserialize as `{function_id, data?, timeout_ms?}` returns a `deserialization_error`.

`timeout_ms` overrides the default 30s deadline for this call only — pair small timeouts with a remote that's known-fast, or larger timeouts with a remote that does heavy work.

## Outputs

`bridge.invoke` returns the remote function's response value **directly** — no envelope wraps it. Whatever the remote function returned (object, array, scalar, `null`) is what the caller sees.

```json
// Example: when the remote function returned `{ "user": { "id": "u_1" } }`
{ "user": { "id": "u_1" } }
```

When the bridge fails (timeout, connection drop, remote rejection, deserialization error), the call returns a `FunctionResult::Failure` with a stable `code` field:

| `code`                  | When                                                                      |
|-------------------------|---------------------------------------------------------------------------|
| `"deserialization_error"` | The input payload didn't match `{function_id, data?, timeout_ms?}`.     |
| `"bridge_error"`        | The remote rejected the call, the WebSocket disconnected mid-call, or the deadline elapsed. The `message` field carries the underlying error from the remote / transport. |

# `bridge.invoke_async`

## Inputs

Same shape as `bridge.invoke`, except `timeout_ms` is **not honored** — the call returns as soon as the worker has handed the message off to the WebSocket send queue:

```json
{
  "function_id": "engine::log::info",                  // required. Remote function id.
  "data":        { "message": "hello from local" }     // optional. Payload forwarded to the remote function.
}
```

If `timeout_ms` is supplied it is ignored — fire-and-forget has no deadline by design.

## Outputs

`bridge.invoke_async` returns no value (`FunctionResult::NoResult`). On the wire the response is `null`. Callers that `await` it should treat success as "the message was queued for delivery" — **not** "the remote handler ran" or "the remote handler succeeded." If the remote later rejects the call, errors are logged on the bridge worker but never surfaced to the caller.

A failure return only happens when the local hand-off itself fails. Use the same `code` table as `bridge.invoke`: `deserialization_error` when the input payload doesn't match `{function_id, data?}`, and `bridge_error` when transport hand-off fails (broken WebSocket, queue full, or any other adapter-level rejection before the message reaches the wire). Once the message reaches the wire, `bridge.invoke_async` is fire-and-forget.

# Worked example

Synchronous remote call (waits for the remote function's response, bounded by `timeout_ms`):

```json
{
  "function_id": "engine::log::info",
  "data":        { "message": "hello from local" },
  "timeout_ms":  5000
}
```

Two patterns reach for these functions:

- **Synchronous remote call.** `bridge.invoke` with the payload above returns the remote function's value directly. Use `timeout_ms` to bound the call when the remote may be slow; the default 30 s is rarely the right deadline for a UI-fronted handler.
- **Fire-and-forget remote dispatch.** `bridge.invoke_async` with the same payload (minus `timeout_ms`, which is ignored) hands the message to the WebSocket and returns. Use it for telemetry, audit logs, mirrored writes — anywhere the caller doesn't need the remote response and shouldn't block on it.

For runnable scaffolds (TypeScript, Python, Rust) plus the `forward:` / `expose:` config patterns that wrap these calls in stable local function ids, see the bridge worker source and the SDK usage examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- `iii-bridge` worker config — `url` (remote WebSocket URL), `service_id` / `service_name` (registered with the remote), `expose:` (functions this engine exposes outward), `forward:` (local-alias entries that proxy to remote functions). Use `forward:` when the same `(local, remote)` pair is called many times so the local alias becomes a stable id.
- The remote engine's own functions — `bridge.invoke` is a generic transport; the actual contract per call is the remote function's input/output shape, not anything this worker defines.
