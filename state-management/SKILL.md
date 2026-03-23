---
name: state-management
description: >-
  Manages distributed key-value state across iii functions using state::get,
  state::set, state::update, and state::delete. Use when persisting data,
  sharing context between functions, or building stateful workflows.
---

# State Management

Comparable to: Redis, DynamoDB

## Key Concepts

- State is a distributed key-value store shared across all functions in the system
- Access state via built-in functions: `state::get`, `state::set`, `state::update`, `state::delete`, `state::list`
- Data is organized by `scope` (namespace) and `key`
- Atomic updates via `state::update` with `ops` array (set path, increment value)
- The `StateModule` with `KvStore` adapter must be enabled in iii-config.yaml

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `init(url)` | Connect worker to engine |
| `trigger({ function_id: 'state::set', payload })` | Write state |
| `trigger({ function_id: 'state::get', payload })` | Read state |
| `trigger({ function_id: 'state::update', payload })` | Atomic update |
| `trigger({ function_id: 'state::delete', payload })` | Delete state |
| `trigger({ function_id: 'state::list', payload })` | List keys in scope |

## Common Patterns

- `trigger({ function_id: 'state::set', payload: { scope, key, value } })` -- write
- `trigger({ function_id: 'state::get', payload: { scope, key } })` -- read
- `trigger({ function_id: 'state::update', payload: { scope, key, ops } })` -- atomic update
- `trigger({ function_id: 'state::delete', payload: { scope, key } })` -- delete
- Update ops: `{ type: 'set', path: 'field', value: 'new' }` or `{ type: 'increment', path: 'count', by: 1 }`

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with TypeScript, Python, Rust examples, update operations, and module config.

## Pattern Boundaries

- If the task reacts to state changes automatically, use `state-reactions`.
- If the task needs ordered state mutations, combine with `queue-processing` (FIFO).
- State is shared across all functions -- use scoped keys to avoid collisions.
- Stay with `state-management` when the goal is reading, writing, or updating persistent data.
