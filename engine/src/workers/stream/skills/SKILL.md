---
name: iii-stream
description: >-
  Durable real-time streams with a CRUD function surface plus engine-hosted
  sources called on item changes and WebSocket subscriber lifecycle — reach for it to
  build live backends without polling. iii-stream is deprecated (iii-stream) and
  will be removed in an upcoming release. Behavior is unchanged for
  now. Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams
---

# iii-stream

> **Deprecated.** iii-stream is deprecated (iii-stream) and will be removed in an upcoming release.
> Behavior is unchanged for now. Migration guide:
> https://iii.dev/docs/upgrading/migrate-from-streams

The `iii-stream` worker stores real-time data as a three-level hierarchy (`stream_name` -> `group_id` -> `item_id`) in the configured adapter and exposes two surfaces: a CRUD-shaped `stream::*` function namespace for reading and writing items, and engine-hosted sources (`stream::on-changed`, `stream::on-join`, `stream::on-leave`) the engine calls on data changes and on WebSocket subscriber connect/disconnect. Reactive backends are built by observing those functions rather than by polling.

A write persists the new value, calls `stream::on-changed` (its observers' targets run asynchronously, so the originating call returns before they finish), and broadcasts the change to every WebSocket client subscribed to that `(stream_name, group_id)`. Authorize subscriptions with the worker's `auth_function` config, which runs once per WebSocket handshake and stamps a `context` value into every join/leave event.

Adapters: `kv` (default; in-memory or file-backed; single-instance only, no cross-process fan-out) or `redis` (required for multi-instance fleets that need real-time fan-out). Browser and client subscriptions use the Browser SDK (`iii-browser-sdk`), which subscribes to `stream` changes over a single engine WebSocket. Connecting directly to the stream port (`ws://host:{port}/stream/{stream_name}/{group_id}`) is deprecated in favor of the Browser SDK.

## When to Use

- A stream change should kick off side effects elsewhere (derived projections, audit logs, notifications) without polling `stream::list`.
- You need to gate WebSocket subscriptions server-side (`auth_function`) instead of trusting client-side filtering.
- You want server-side reactions to subscriber connect/disconnect for presence counters, per-subscription rate buckets, or audit trails.
- An item needs atomic partial updates that concurrent writers would otherwise race on.

## Boundaries

- Not a general key/value store — data is stream-shaped (`stream_name`/`group_id`/`item_id`). Use the standalone `state` worker for scope/key values.
- The default `kv` adapter does not fan out across processes; multi-instance fleets that need real-time broadcast must use `redis`.
- `stream::on-join` and `stream::on-leave` are notifications, not authorization gates: their observers' return values are ignored.
- Observer targets run after the originating write returns; a target failure neither rolls back the write nor surfaces to the caller.

## Functions

- `stream::set` — persist an item and broadcast a create/update event; returns the previous value alongside the new one.
- `stream::update` — atomically apply ordered `set`/`merge`/`increment`/`decrement`/`append`/`remove` ops to an existing item.
- `stream::delete` — remove an item and broadcast a delete event carrying the removed value.
- `stream::send` — broadcast a transient event to a group's subscribers without persisting it (typing indicators, cursor positions).
- `stream::get` — read one item by its full `(stream, group, item)` triple.
- `stream::list` — enumerate items in a group.
- `stream::list_groups` — enumerate groups in a stream.
- `stream::list_all` — enumerate every stream's metadata.

## Reacting to stream activity

Observe a `stream::on-*` source when a function should run automatically on stream activity — without polling `stream::list`. Three sources cover two concerns: `stream::on-changed` reacts to data changes, while `stream::on-join` / `stream::on-leave` react to WebSocket subscriber lifecycle.

Reach for them when:

- A `set`/`update`/`delete`/`send` should drive a projection, audit log, or downstream notification.
- Subscriptions need paired setup/teardown (`stream::on-join` + `stream::on-leave`).

If you only need the current value on demand, call `stream::get` instead of observing.

### How to observe

1. Register a handler: `iii.registerFunction('presence::on-change', handler)`.
2. Observe the source:

```typescript
iii.observe(
  {
    function_id: 'stream::on-changed',
    filters: {
      'input.stream_name': { $eq: 'presence' },
      'input.group_id': { $eq: 'room-1' },    // optional. Omit to match every group.
      'input.id': { $eq: 'user-123' },        // optional. Omit to match every item.
    },
  },
  'presence::on-change',
)
```

Mutations that call `stream::on-changed`: `stream::set`, `stream::update`, `stream::delete`, `stream::send`. Reads do not.

For the event shape, call `engine::functions::info { function_id: "stream::on-changed" }`: its `request_format` is the event.
