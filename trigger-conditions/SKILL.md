---
name: trigger-conditions
description: >-
  Run a condition function before trigger handlers execute. Use when you need
  to gate event processing on business rules, filter events by criteria, or
  conditionally skip handler execution based on event data.
---

# Trigger Conditions

Gate trigger execution with a condition function.

## Key Concepts

- A **Condition Function** is a registered function that returns `true` or `false`
- When a trigger fires, the engine calls the condition function first; the handler runs only if it returns `true`
- Attach a condition to any trigger type via the `condition_function_id` config parameter
- The condition function receives the same event data the handler would receive
- Works with all trigger types: `state`, `http`, `queue`, `cron`, `stream`, `subscribe`

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `registerFunction({ id }, handler)` | Register the condition function (returns boolean) |
| `registerFunction({ id }, handler)` | Register the handler function (runs if condition passes) |
| `registerTrigger({ type, function_id, config: { ..., condition_function_id } })` | Bind trigger with condition |

## Common Patterns

- Register a condition: `registerFunction({ id: 'conditions::is-high-value' }, async (input) => input.new_value?.amount >= 1000)`
- Register a handler: `registerFunction({ id: 'orders::notify-high-value' }, async (input) => { ... })`
- Bind with condition: `registerTrigger({ type: 'state', function_id: 'orders::notify-high-value', config: { scope: 'orders', key: 'status', condition_function_id: 'conditions::is-high-value' } })`
- Condition returns `true` -- handler executes
- Condition returns `false` -- handler is skipped silently

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with TypeScript, Python, and Rust examples.

## Pattern Boundaries

- For function registration and trigger binding, see `functions-and-triggers`
- For state change triggers specifically, see `state-reactions`
- For trigger invocation modes (sync/void/enqueue), see `trigger-actions`
