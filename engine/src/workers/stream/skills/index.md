---
type: index
title: iii-stream
---

# iii-stream

Durable real-time streams. Data is organized as a three-level hierarchy
(`stream_name` → `group_id` → `item_id`) and lives in the configured
adapter (`kv` or `redis`). The worker has two equally important
surfaces — a CRUD-shaped function namespace for reading and writing
items, and a set of **reactive triggers** (`stream`, `stream:join`,
`stream:leave`) that fire automatically on data changes and on
WebSocket subscriber lifecycle events. Reactive backends are built by
attaching trigger handlers to those events rather than by polling.

Writes through the worker do three things in sequence: persist the new
value, dispatch the registered `stream::*` triggers (fire-and-forget on
a spawned task), and broadcast the change to every WebSocket client
subscribed to that `(stream_name, group_id)`. The trigger task is
async with respect to the original call — a `stream::set` returns
before the trigger handlers complete, but persistence and the
adapter's `emit_event` broadcast are synchronous within the call.

The `stream:join` trigger doubles as an **authorization gate**:
returning `{ unauthorized: true }` from a join handler rejects the
incoming subscription before any data flows. Pair this with the
worker's `auth_function` config (which runs once per WebSocket
handshake and stamps a `context` value into every join/leave event) to
build per-connection authorization.

The worker config block: `port` (default `3112`), `host` (default
`0.0.0.0`), `auth_function` (function id invoked on every WebSocket
handshake; receives `{ headers, path, query_params, addr }` and
returns `{ context: <any> }`), and `adapter` — either `kv` (default;
in-memory or file-backed; **single-instance only**, no cross-instance
fan-out) or `redis` (`redis_url: ${REDIS_URL:redis://localhost:6379}`;
required for multi-instance fleets that need real-time fan-out across
processes). Clients connect at
`ws://host:{port}/stream/{stream_name}/{group_id}` and optionally
`/{item_id}` for item-scoped subscriptions.

- **Functions** (`stream::*`) — the read/write surface for stream items. CRUD plus an atomic `update` for op-list mutation and a transient `send` for ephemeral broadcasts.
- **Reactive triggers** (`stream`, `stream:join`, `stream:leave`) — fire on item changes and on WebSocket subscribe/unsubscribe events. The whole point of the worker for reactive backends.

## How-tos

### `stream::*` functions

- [`stream::set`](iii://iii-stream/stream/set) — persist an item and broadcast a `create`/`update` event. Returns the previous value alongside the new one so callers can branch on insert vs. overwrite without a follow-up `get`.
- [`stream::update`](iii://iii-stream/stream/update) — atomically apply a list of `set`/`merge`/`increment`/`decrement`/`append`/`remove` ops to an existing item. Use instead of read-modify-write when concurrent writers can race on the same item.
- [`stream::delete`](iii://iii-stream/stream/delete) — remove an item and broadcast a `delete` event carrying the removed value. No-ops cleanly when the item didn't exist.
- [`stream::send`](iii://iii-stream/stream/send) — broadcast a custom event to a group's subscribers **without** persisting it. Use for ephemeral signals (typing indicators, cursor positions) that shouldn't survive a reload.
- [`stream::get`](iii://iii-stream/stream/get) — read one item by its full `(stream, group, item)` triple. Returns `null` when the item isn't there.
- [`stream::list`, `stream::list_groups`, `stream::list_all`](iii://iii-stream/stream/listing) — three read-only enumerations: items in a group, groups in a stream, and every stream's metadata. One file because the same workflow (`list_all` → pick a stream → `list_groups` → pick a group → `list`) chains them.

### `stream` triggers

- [React to stream item changes and subscriber lifecycle](iii://iii-stream/stream/reactive-triggers) — register handlers for the three reactive trigger types: `stream` (fires on every `set`/`update`/`delete`/`send` matching a `(stream_name, group_id, item_id)` filter), `stream:join` (authorization gate plus per-subscription setup), and `stream:leave` (paired teardown).
