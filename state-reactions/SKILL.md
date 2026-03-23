---
name: state-reactions
description: >-
  Auto-triggers functions when state changes using registerTrigger type:'state'.
  Use when building reactive systems that respond to data mutations, event
  sourcing patterns, or change-driven workflows.
---

# State Reactions

Comparable to: DynamoDB Streams, Firebase Realtime Database triggers, Redis Keyspace Notifications

## Key Concepts

- A **State Trigger** watches a `scope` (and optionally a `key`) for changes
- When state is written via `state::set` or `state::update`, matching triggers fire automatically
- The handler receives `{ key, new_value, old_value, scope, event_type }`
- Optional `condition_function_id` filters which changes invoke the handler
- Requires the `StateModule` with `KvStore` adapter in iii-config.yaml

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `init(url)` | Connect worker to engine |
| `registerFunction({ id }, handler)` | Define the reaction handler |
| `registerTrigger({ type: 'state', function_id, config })` | Watch state for changes |
| `config: { scope, key? }` | Scope to watch, optional key filter |
| `trigger({ function_id: 'state::set', payload })` | Write state (triggers reactions) |

## Common Patterns

- `registerTrigger({ type: 'state', function_id, config: { scope: 'orders' } })` -- watch all keys in scope
- `registerTrigger({ type: 'state', function_id, config: { scope: 'orders', key: 'status' } })` -- watch specific key
- Handler receives `event.new_value`, `event.old_value`, `event.key`, `event.scope`, `event.event_type`
- Combine with `condition_function_id` for conditional reactions
- Chain reactions: a state change handler can write state that triggers another handler

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with TypeScript, Python, Rust examples and condition functions.

## Pattern Boundaries

- If the task needs to read/write state without reacting to changes, use `state-management`.
- If the task needs time-based triggers, use `cron-scheduling`.
- If the task needs manual/explicit invocation chains, use plain `trigger()` calls.
- Stay with `state-reactions` when the goal is automatic, event-driven responses to data changes.
