---
name: iii-stream
description: >-
  Durable real-time streams with a CRUD function surface plus reactive triggers
  that fire on item changes and WebSocket subscriber lifecycle — reach for it to
  build live backends without polling.
---

# iii-stream

The `iii-stream` worker stores real-time data as a three-level hierarchy (`stream_name` -> `group_id` -> `item_id`) in the configured adapter and exposes two surfaces: a CRUD-shaped `stream::*` function namespace for reading and writing items, and reactive trigger types (`stream`, `stream:join`, `stream:leave`) that fire on data changes and on WebSocket subscriber connect/disconnect. Reactive backends are built by binding handlers to those triggers rather than by polling.

A write persists the new value, dispatches matching `stream` triggers on a spawned task (fire-and-forget, so the originating call returns before handlers finish), and broadcasts the change to every WebSocket client subscribed to that `(stream_name, group_id)`. The `stream:join` trigger doubles as an authorization gate: returning `{ unauthorized: true }` from its handler rejects the subscription before any data flows. Pair it with the worker's `auth_function` config, which runs once per WebSocket handshake and stamps a `context` value into every join/leave event.

Adapters: `kv` (default; in-memory or file-backed; single-instance only, no cross-process fan-out) or `redis` (required for multi-instance fleets that need real-time fan-out). Clients connect at `ws://host:{port}/stream/{stream_name}/{group_id}` and optionally `/{item_id}`.

## When to Use

- A stream change should kick off side effects elsewhere (derived projections, audit logs, notifications) without polling `stream::list`.
- You need to gate WebSocket subscriptions server-side instead of trusting client-side filtering.
- You want server-side reactions to subscriber connect/disconnect for presence counters, per-subscription rate buckets, or audit trails.
- An item needs atomic partial updates that concurrent writers would otherwise race on.

## Boundaries

- Not a general key/value store — data is stream-shaped (`stream_name`/`group_id`/`item_id`). Use `iii-state` for scope/key values.
- The default `kv` adapter does not fan out across processes; multi-instance fleets that need real-time broadcast must use `redis`.
- `stream:leave` is not an authorization gate — the subscription is already gone by the time it fires, and its return value is ignored.
- Trigger handlers run after the originating write returns; a handler failure neither rolls back the write nor surfaces to the caller.

## Functions

- `stream::set` — persist an item and broadcast a create/update event; returns the previous value alongside the new one.
- `stream::update` — atomically apply ordered `set`/`merge`/`increment`/`decrement`/`append`/`remove` ops to an existing item.
- `stream::delete` — remove an item and broadcast a delete event carrying the removed value.
- `stream::send` — broadcast a transient event to a group's subscribers without persisting it (typing indicators, cursor positions).
- `stream::get` — read one item by its full `(stream, group, item)` triple.
- `stream::list` — enumerate items in a group.
- `stream::list_groups` — enumerate groups in a stream.
- `stream::list_all` — enumerate every stream's metadata.

## Reactive triggers

Bind a `stream`-family trigger when a function should run automatically on stream activity — without polling `stream::list`. Three types cover two concerns: `stream` reacts to data changes, while `stream:join` / `stream:leave` react to WebSocket subscriber lifecycle.

Reach for them when:

- A `set`/`update`/`delete`/`send` should drive a projection, audit log, or downstream notification.
- Subscriptions need server-side authorization (`stream:join`) or paired setup/teardown (`stream:join` + `stream:leave`).

If you only need the current value on demand, call `stream::get` instead of binding a trigger.

### How to bind

1. Register a handler: `iii.registerFunction('presence::on-change', handler)`.
2. Register the trigger:

```typescript
iii.registerTrigger({
  type: 'stream',
  function_id: 'presence::on-change',
  config: {
    stream_name: 'presence',  // required for `stream`; the worker indexes triggers by it.
    group_id: 'room-1',       // optional. Empty/omitted = match every group.
    item_id: 'user-123',      // optional. Empty/omitted = match every item.
    // condition_function_id is also supported.
  },
})
```

`stream` requires a non-empty `stream_name`; `group_id` / `item_id` use empty-as-wildcard, exact-match-otherwise semantics. `stream:join` and `stream:leave` take no config fields — branch inside the handler on the event. A `stream:join` handler returning `{ unauthorized: true }` rejects the subscription; any other return allows it. Mutations that fire `stream`: `stream::set`, `stream::update`, `stream::delete`, `stream::send`. Reads do not.

For the event payload shape, call `iii get function info` on the trigger type or handler function id.
