---
name: iii-bridge
description: >-
  Connect this engine to another iii engine over a long-lived WebSocket so
  functions call across the boundary. Wire stable ids with `forward:`/`expose:`;
  `bridge.invoke` is the ad-hoc escape hatch.
---

# iii-bridge

The `iii-bridge` worker connects this iii engine to another iii instance over `iii-sdk` so functions on either side can call across the boundary. It opens a single outbound WebSocket to the configured `url`, registers with the remote using `service_id`, and stays open for the engine's lifetime — bridging is request/response over that long-lived connection. There are no trigger types.

The worker is configuration-driven. The primary surface is two list-shaped config fields (`forward:` and `expose:`) that wire stable function ids on both sides; once configured, callers reach across the bridge by invoking those stable ids with the normal `iii.trigger({ function_id, payload })` — no bridge-specific call shape. Two functions (`bridge.invoke`, `bridge.invoke_async`) are also registered as ad-hoc escape hatches for the rare case where the remote function id is dynamic at runtime.

## When to Use

- Two iii engines need to call each other's functions over a stable, long-lived connection.
- You want a remote function to appear as a local id (`forward:`) so the bridge is invisible at the call site.
- You want to expose specific local functions to a remote engine (`expose:`).
- The remote function id is dynamic, or you are prototyping / probing connectivity — reach for the ad-hoc `bridge.invoke` functions.

## Boundaries

- Prefer `forward:` / `expose:` aliases over `bridge.invoke`; the escape hatches are for dynamic ids and one-offs, not the default path.
- `bridge.invoke_async` is fire-and-forget — it ignores `timeout_ms`, returns no value, and a later remote rejection is logged but never surfaced to the caller.
- Forward aliases and exposed ids are operator-wired per deployment in `iii-config.yaml`; they are not stable across deployments and are documented alongside the worker config, not here.
- Failures return a `FunctionResult::Failure` with a stable `code` (`deserialization_error` or `bridge_error`); a successful async return only means the message was queued, not that the remote ran.

## Functions

- `bridge.invoke` — call a remote `function_id` and wait for its return value (returned directly, no envelope); honors an optional `timeout_ms` (default `30000`).
- `bridge.invoke_async` — hand a remote call to the WebSocket send queue and return immediately; `timeout_ms` is ignored and no remote response is surfaced.

Both take `{ function_id, data?, timeout_ms? }`. Reach for them only when a `forward:` alias is wrong or impossible; for repeated calls to the same `(local, remote)` pair, configure a `forward:` alias and call the local id instead.

## Configuration

- `url` — WebSocket URL of the remote iii instance (default `${III_URL:ws://0.0.0.0:49134}`).
- `service_id` / `service_name` — identifier (and human-readable name) registered with the remote.
- `expose: [{ local_function, remote_function? }]` — functions on this engine the remote may call; `remote_function` is the path the remote invokes (defaults to `local_function`). Registered with the remote at initialize time.
- `forward: [{ local_function, remote_function, timeout_ms? }]` — local aliases that proxy outbound to a remote function. The worker registers `local_function` on this engine so any caller reaches the remote's `remote_function`; `timeout_ms` overrides the per-call deadline (default `30000`).
