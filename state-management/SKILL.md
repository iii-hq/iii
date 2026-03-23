---
name: state-management
description: >-
  Reads and writes distributed key-value state across functions. Use when
  persisting data between invocations, managing sessions, or building a data
  layer.
---

# State Management

Comparable to: Redis, DynamoDB, Memcached

## Key Concepts

Use the concepts below when they fit the task. Not every state operation needs all of them.

- State is a **scoped key-value store** accessed via built-in trigger functions
- **state::set** writes a value; **state::get** reads it (returns `null` for missing keys)
- **state::list** retrieves all keys in a scope; **state::delete** removes a key
- **state::update** performs a **partial merge** using an `ops` array for fine-grained changes
- Payloads use `scope`, `key`, and `value` to address state entries
- State is shared across all functions — use meaningful scope names to avoid collisions

## Architecture

    Function
      → trigger('state::set', { scope, key, value })
      → trigger('state::get', { scope, key })
      → trigger('state::update', { scope, key, ops })
      → trigger('state::delete', { scope, key })
      → trigger('state::list', { scope })
        → StateModule → KvStore / Redis adapter

## iii Primitives Used

| Primitive                                                          | Purpose                              |
| ------------------------------------------------------------------ | ------------------------------------ |
| `trigger({ function_id: 'state::set', payload })`                  | Write a value to state               |
| `trigger({ function_id: 'state::get', payload })`                  | Read a value from state              |
| `trigger({ function_id: 'state::list', payload })`                 | List all keys in a scope             |
| `trigger({ function_id: 'state::delete', payload })`               | Remove a key from state              |
| `trigger({ function_id: 'state::update', payload: { ops } })`      | Partial merge with operations array  |

## Reference Implementation

See [references/reference.js](references/reference.js) for the full working example — functions that read, write, update, and delete state entries across a shared scope.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `trigger({ function_id: 'state::set', payload: { scope, key, value } })` — write state
- `trigger({ function_id: 'state::get', payload: { scope, key } })` — read state (returns `null` if missing)
- `trigger({ function_id: 'state::update', payload: { scope, key, ops } })` — partial merge
- `trigger({ function_id: 'state::list', payload: { scope } })` — enumerate keys
- `trigger({ function_id: 'state::delete', payload: { scope, key } })` — remove entry
- `const logger = new Logger()` — structured logging

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Name scopes after your domain (e.g. `user-sessions`, `order-data`, `config`)
- Use `state::get` with a `null` check to handle missing keys gracefully
- Use `state::update` with `ops` for partial updates instead of read-modify-write cycles
- Combine with `queue-processing` to persist results after async job completion

## Pattern Boundaries

- If the task needs reactive side effects when state changes, prefer `state-reactions`.
- If the task needs real-time client push when data updates, prefer `realtime-streams`.
- Stay with `state-management` when the primary need is reading and writing persistent key-value data.
