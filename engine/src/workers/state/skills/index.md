---
type: index
title: iii-state
---

# iii-state

The `iii-state` worker is a server-side key-value store. Values are addressed by a `scope` (namespace) and a `key`, shared across every worker connected to the engine, and persisted through a pluggable adapter (`kv`, `redis`, or `bridge`). It does not push updates to clients — reactivity is delivered by a `state` trigger type that fires on every create, update, and delete.

Callers reach the store through six engine functions invoked with `iii.trigger({ function_id: 'state::...', payload })`. Use them when you need shared state between functions without standing up a separate database, when a counter or per-entity document needs atomic partial updates, or when downstream functions need to react to data changes.

- **`state::*` functions** (`iii-state::state::*`) — CRUD and atomic partial-update operations against the configured adapter.
- **`state` trigger type** — fires `state:created`, `state:updated`, or `state:deleted` events after every successful mutation. Triggers register via `iii.registerTrigger({ type: 'state', ... })` with optional `scope`, `key`, and `condition_function_id` filters and are not invoked through `state::*`. See [React to state changes](iii://iii-state/state/reactive-triggers).

Adapter notes: `kv` is the default and supports `in_memory` or `file_based` persistence; `redis` proxies state to a Redis backend; `bridge` forwards operations to a remote III Engine. The function surface is identical across adapters.

## How-tos

### `state::*`

- [`state::set`](iii://iii-state/state/set) — write or replace the value at a `scope`/`key` and fan out a `state:created` or `state:updated` event.
- [`state::get`](iii://iii-state/state/get) — read one value when you already know the `scope` and `key`.
- [`state::delete`](iii://iii-state/state/delete) — remove a key and emit a `state:deleted` event with the prior value.
- [`state::update`](iii://iii-state/state/update) — apply ordered atomic ops (`set`, `merge`, `increment`, `decrement`, `append`, `remove`) instead of a read-modify-write cycle.
- [`state::list`](iii://iii-state/state/list-and-groups) — enumerate every value stored in a scope.
- [`state::list_groups`](iii://iii-state/state/list-and-groups) — discover which scopes contain data.

### `state` triggers

- [React to state changes](iii://iii-state/state/reactive-triggers) — register a handler and `state` trigger to run side effects when `state::set`, `state::update`, or `state::delete` mutates a watched `scope`/`key`.
