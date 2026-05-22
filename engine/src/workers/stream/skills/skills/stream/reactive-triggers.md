---
type: how-to
trigger_type: stream
title: React to stream item changes and subscriber lifecycle
---

# When to use

The `iii-stream` worker exposes three reactive trigger types — registered through `iii.registerTrigger({ type, function_id, config })` rather than through `stream::*`-style engine functions. They cover two distinct concerns: **data changes** to stream items, and **subscriber lifecycle** events on WebSocket clients.

| Trigger type    | Fires on                                                                                       | Typical use                                                                                              |
|-----------------|------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------|
| `stream`        | Successful `stream::set`, `stream::update`, `stream::delete`, or `stream::send` matching the configured filter. | Server-side change watcher: derived projections, audit logs, downstream notifications.                  |
| `stream:join`   | A WebSocket client subscribes to a `(stream_name, group_id, [id])`.                            | Authorize the subscription (return `{ unauthorized: true }` to reject) **and/or** record per-connection setup. |
| `stream:leave`  | A WebSocket client unsubscribes (explicit `leave` message or socket close).                    | Cleanup paired with `stream:join`: decrement counters, release per-subscription resources, broadcast "user-left" signals. |

Reach for these when:

- A stream change should kick off side effects in another function (audit logs, projections, notifications) without polling `stream::list`.
- You need to gate WebSocket subscriptions server-side — verifying that the connection's `auth_function` context has access to the requested `(stream_name, group_id)` — rather than trusting client-side filtering.
- You want server-side reactions to subscriber connect/disconnect for presence counters, per-subscription rate buckets, or audit trails.

Use [`stream::get`](iii://iii-stream/stream/get) instead when you only need to **pull** the current value on demand. Use [`stream::send`](iii://iii-stream/stream/send) inside a trigger handler when the side effect is "broadcast a transient notification" — `set` already broadcasts; `send` lets you emit an additional, distinct event.

Prerequisite: the `iii-stream` worker must be enabled in `config.yaml`. Handlers and triggers are registered from a connected worker via `iii.registerFunction` and `iii.registerTrigger` — not through the `stream::*` engine functions.

# Inputs

Registration is the same two-step pattern for all three trigger types: define the handler function, then bind it to the trigger.

## Handler function

Register any function id. The handler receives the event payload documented in Outputs (same shape the engine passes to `condition_function_id` when configured).

```json
// iii.registerFunction — handler id only; no engine payload.
{ "id": "presence::on-change" }
```

## `stream` trigger registration (data-change)

```json
{
  "type":        "stream",                      // required. Must be exactly "stream".
  "function_id": "presence::on-change",         // required. Handler invoked when the trigger matches.
  "config": {
    "stream_name":           "presence",        // required. Non-empty. Registration fails synchronously if missing.
    "group_id":              "room-1",          // optional. Empty string or omitted = match every group within the stream.
    "item_id":               "user-123",        // optional. Empty string or omitted = match every item within the (filtered) group.
    "condition_function_id": "auth::should-fire" // optional. Engine invokes this with the event; handler runs only on truthy.
  }
}
```

`type`, `function_id`, and `config.stream_name` are required. `stream_name` cannot be empty — the worker indexes triggers by stream name and unwraps the field, so registration without it fails synchronously. `group_id` and `item_id` use **empty-string-as-wildcard** semantics: an omitted or empty value means "match anything." The match is exact equality otherwise — no glob, no prefix matching.

| Config field            | When omitted or empty                          | When set                                                         |
|-------------------------|------------------------------------------------|------------------------------------------------------------------|
| `group_id`              | Matches every group in the stream              | Matches only events whose `group_id` equals this string          |
| `item_id`               | Matches every item in the matched group(s)     | Matches only events whose `id` (the item id) equals this string  |
| `condition_function_id` | Handler runs whenever stream/group/item match  | Engine invokes the named function with the event; handler is skipped on `false` or condition error |

## `stream:join` trigger registration (subscription start, with optional auth gate)

```json
{
  "type":        "stream:join",                 // required.
  "function_id": "presence::on-join",           // required.
  "config":      {}                             // takes no fields. Branch inside the handler on the event's stream_name/group_id/id.
}
```

`stream:join` takes **no config fields**. It fires for every subscription on every stream. To narrow, branch inside the handler.

The handler's return value is special — `{ "unauthorized": true }` rejects the subscription before any data flows; anything else (or no return) lets the subscription proceed. See **Outputs** below.

## `stream:leave` trigger registration (subscription teardown)

```json
{
  "type":        "stream:leave",
  "function_id": "presence::on-leave",
  "config":      {}                             // takes no fields. Same shape as stream:join's config.
}
```

`stream:leave` also takes no config and is **not** an authorization gate — by the time the handler fires, the subscription is already gone. The return value is ignored.

# Outputs

The handler payloads differ between the data-change trigger and the lifecycle pair.

## `stream` event payload (`StreamWrapperMessage`)

```json
{
  "type":       "stream",                       // always the literal "stream"
  "timestamp":  1716220800000,                  // milliseconds since epoch (UTC)
  "streamName": "presence",                     // camelCase — the source uses serde rename
  "groupId":    "room-1",
  "id":         "user-123",                     // null on stream::send calls that omit the `id` field (group-wide broadcast)
  "event": {
    "type": "create",                           // discriminant: "create" | "update" | "delete" | "event"
    "data": { "online": true, "name": "Alice" }
  }
}
```

- The wrapper field names use camelCase (`streamName`, `groupId`) because they're declared with `#[serde(rename = ...)]` in the source.
- `event.type` is the discriminant. The four shapes:
  - `"create"` — fired by `stream::set` / `stream::update` when the item didn't exist before. `event.data` is the new value.
  - `"update"` — fired by `stream::set` / `stream::update` when the item existed. `event.data` is the new value.
  - `"delete"` — fired by `stream::delete`. `event.data` is the value that was removed.
  - `"event"` — fired by `stream::send`. `event.data` is wrapped one extra level: `{ type: <user_type>, data: <user_data> }`.
- `id` is `null` only on `stream::send` calls that omit the `id` field. On `set`/`update`/`delete` it always carries the `item_id`.

The handler's return value is **ignored**. Errors are logged but do not affect the original `stream::*` call site — trigger fan-out runs on a spawned task **after** the originating call returns.

## `stream:join` and `stream:leave` event payload (`StreamJoinLeaveEvent`)

```json
{
  "subscription_id": "f8e2c0a4-...",            // worker-issued, unique per subscribe call
  "stream_name":     "presence",                // snake_case — different from the data-trigger payload
  "group_id":        "room-1",
  "id":              "user-123",                // null when subscribing to the whole (stream, group) rather than a single item
  "context":         { "user_id": "alice@example.com" }  // whatever the worker config's auth_function returned for the connection; null if no auth_function is set
}
```

- All field names are snake_case here — do not confuse with the data-trigger's camelCase wrapper.
- `subscription_id` matches between a `stream:join` event and its paired `stream:leave` event for the same subscription. Use it as a join key for per-subscription state.
- `context` is captured at WebSocket handshake time and the same value flows into every join/leave event for the lifetime of the connection.

For `stream:join` only, the handler's return value drives authorization:

```json
{ "unauthorized": true }                        // reject the subscription
{ "unauthorized": false }                       // allow (same as omitting)
{}                                              // allow (default)
null                                            // allow (default)
```

Only the `unauthorized` field is read; any other shape — including a return value that fails to parse — is treated as "allow." Errors in the join handler are logged but do not block the subscription.

For `stream:leave`, the return value is ignored.

# Worked example

Watch every change in `presence` / `room-1` so a handler fires on every `create`/`update`/`delete`/`event`:

```json
{
  "type":        "stream",
  "function_id": "presence::on-change",
  "config":      { "stream_name": "presence", "group_id": "room-1" }
}
```

Three patterns build on this base:

- **Watch a `(stream_name, group_id)` for changes.** The registration above; branch inside the handler on `event.event.type` to distinguish `create`/`update`/`delete`/`event`.
- **Authorize subscriptions before any data flows.** Register a `stream:join` handler that inspects the event's `context` / `group_id` and returns `{ "unauthorized": true }` to reject; pair with a `stream:leave` handler matched on `subscription_id` for symmetric cleanup.
- **Drive a derived projection.** From inside a `stream` handler, call `stream::set` / `stream::update` against a different `(stream_name, group_id)` to maintain counters, search indexes, or audit logs; the originating write has already returned, so a slow projection cannot delay or block writers.

For runnable scaffolds covering these patterns end-to-end (TypeScript, Python, and Rust), see the stream worker source and the SDK usage examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [`stream::set`](iii://iii-stream/stream/set), [`stream::update`](iii://iii-stream/stream/update), [`stream::delete`](iii://iii-stream/stream/delete), [`stream::send`](iii://iii-stream/stream/send) — the four functions whose calls the `stream` trigger reacts to.
- [`stream::get`](iii://iii-stream/stream/get) — read on demand instead of registering a trigger.
- `auth_function` (worker config) — a function id set on the `iii-stream` worker; the engine invokes it once per WebSocket handshake with `{ headers, path, query_params, addr }`, expects `{ context: <any> }` back, and stamps that `context` into every subsequent `stream:join` / `stream:leave` event for the connection.
