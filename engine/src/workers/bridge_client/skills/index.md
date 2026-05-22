---
type: index
title: iii-bridge
---

# iii-bridge

Connect this iii engine instance to another iii instance over `iii-sdk` so functions on either side can call across the boundary. The worker opens a single outbound WebSocket connection to the configured `url`, registers itself with the remote engine using `service_id`, and stays open for the engine's lifetime — bridging is request/response over that long-lived connection. There are no trigger types.

The worker is **configuration-driven**. The primary surface is two list-shaped config fields (`forward:` and `expose:`) that wire stable function ids on both sides; once configured, callers reach across the bridge by invoking those stable ids with the standard `iii.trigger({ function_id, payload })` — no bridge-specific call shape required. Two engine functions (`bridge.invoke`, `bridge.invoke_async`) are also registered as ad-hoc escape hatches for the rare case where the remote function id is dynamic at runtime; they are not the recommended path.

Connection config: `url` (WebSocket URL of the remote iii instance; defaults to `${III_URL:ws://0.0.0.0:49134}`), `service_id` (service identifier registered with the remote), `service_name` (human-readable; defaults to `service_id`).

Two list-shaped fields wire the bridge:

- `expose: [{ local_function, remote_function? }]` — functions registered on **this** engine that the remote should be able to call. `remote_function` is the path the remote will invoke; defaults to `local_function`. The worker registers each entry with the remote at initialize time; remote callers then `iii.trigger({ function_id: remote_function, payload })` and the call lands on this engine.
- `forward: [{ local_function, remote_function, timeout_ms? }]` — local aliases that proxy outbound to a remote function. The worker dynamically registers `local_function` on this engine so any caller can `iii.trigger({ function_id: local_function, payload })` and the call reaches the remote's `remote_function`. `timeout_ms` overrides the per-call deadline (default `30000`).

Forward aliases and exposed functions don't have stable ids across deployments — they're whatever the operator wires in `iii-config.yaml`. This skill bundle covers the worker's purpose and the ad-hoc escape hatch only; the configured aliases are documented per-deployment alongside the worker's config.

- **Configuration-driven** (`forward:` / `expose:`) — the recommended way to bridge. Configure once, call the local alias by its normal id; the bridge is invisible at the call site.
- **Ad-hoc escape hatch** (`bridge.invoke` / `bridge.invoke_async`) — for one-off calls where the remote function id is dynamic at runtime, or for testing the connection. Reach for these only when a forward alias would be wrong.

## How-tos

### `bridge.*`

- [`bridge.invoke` and `bridge.invoke_async`](iii://iii-bridge/bridge/invoke) — ad-hoc remote calls when a `forward:` alias isn't appropriate. Wait for the response with `bridge.invoke` or fire-and-forget with `bridge.invoke_async`. One file because they share the same input shape and only differ in whether the response round-trips.
