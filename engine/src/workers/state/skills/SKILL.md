---
name: iii-state
description: >-
  Shared, adapter-backed key/value store addressed by scope and key, with a
  reactive `state` trigger so other functions can run on every create, update,
  or delete without polling.
---

# iii-state

The `iii-state` worker is a server-side key/value store. Values are addressed by a `scope` (namespace) and a `key`, shared across every worker connected to the engine, and persisted through a pluggable adapter (`kv`, `redis`, or `bridge`). Callers reach the store through six `state::*` functions invoked with `iii.trigger({ function_id: 'state::...', payload })`.

State does not push updates to SDK clients. Reactivity is delivered by a `state` trigger type that fires `state:created`, `state:updated`, or `state:deleted` after every successful mutation, so downstream functions can react to data changes without polling. The `kv` adapter (default) supports `in_memory` or `file_based` persistence; `redis` proxies to a Redis backend; `bridge` forwards operations to a remote iii engine. The function surface is identical across adapters.

## When to Use

- Two or more functions need shared state without standing up a separate database.
- A counter or per-entity document needs atomic partial updates instead of read-modify-write.
- A write in one function should trigger side effects elsewhere (cache invalidation, audit logs, notifications, projections).
- You want a specific `scope`/`key` watched and reacted to only when that slot changes.

## Boundaries

- Schema-free by design — every value is opaque JSON. Use `configuration` when entries need a registered JSON Schema and validation.
- Reads (`state::get`, `state::list`, `state::list_groups`) never fire triggers; only `set`/`update`/`delete` do.
- Trigger delivery is asynchronous and does not roll back the write on handler failure; a delete of a missing key still emits `state:deleted` with a null old value.
- Stream-shaped, broadcast-to-subscribers data belongs in `iii-stream`, not here.

## Functions

- `state::set` — write or replace the value at a `scope`/`key`; fires `state:created` or `state:updated`.
- `state::get` — read one value by `scope` and `key`.
- `state::delete` — remove a key and fire `state:deleted` with the prior value.
- `state::update` — apply ordered atomic ops (`set`, `merge`, `increment`, `decrement`, `append`, `remove`) to the stored value.
- `state::list` — enumerate every value stored in a scope.
- `state::list_groups` — enumerate which scopes currently contain data.

## Reactive triggers

Bind a `state` trigger when a function should run automatically after a value changes — without polling `state::get`. The engine evaluates every registered `state` trigger after a successful `state::set`, `state::update`, or `state::delete` and invokes matching handlers asynchronously.

Reach for it when:

- A write in one worker should drive side effects in another (audit logs, cache invalidation, notifications, projections).
- You want optional gating via `condition_function_id` so the handler only runs when a predicate on the event is truthy.

If you only need the new value inside the same function that wrote it, use the mutator's returned `old_value` / `new_value` instead of binding a trigger.

### How to bind

1. Register a handler: `iii.registerFunction('orders::on-status-change', handler)`.
2. Register the trigger:

```typescript
iii.registerTrigger({
  type: 'state',
  function_id: 'orders::on-status-change',
  config: {
    scope: 'orders',  // optional. Omit to match every scope.
    key: 'status',    // optional. Omit to match every key in the scope.
    // condition_function_id is also supported.
  },
})
```

All `config` fields are optional; tighter filters reduce how often the handler runs. To watch several specific keys, register one trigger per `scope`/`key` pair. Mutations that fire the trigger: `state::set`, `state::update`, `state::delete`.

For the event payload shape, call `iii get function info` on the trigger type or handler function id.
