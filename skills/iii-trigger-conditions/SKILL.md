---
name: iii-trigger-conditions
description: >-
  Registers a boolean condition function and attaches it to triggers via
  condition_function_id so handlers only fire when the condition passes. Use
  when gating triggers on business rules, checking user permissions, validating
  data before processing, filtering high-value orders, rate-limiting events, or
  conditionally skipping handlers based on payload content.
---

# Trigger Conditions

Comparable to: Middleware guards, event filters

## Key Concepts

Use the concepts below when they fit the task. Not every trigger needs a condition.

- A **Condition Function** is a registered function that returns a boolean (`true` or `false`)
- The engine calls the condition function before the handler; the handler runs only if `true`
- Attach a condition to any trigger type via `condition_function_id` in the trigger config
- The condition function receives the same event data as the handler would
- Works with all trigger types: http, durable:subscriber, cron, state, stream, subscribe

## Architecture

When a trigger fires, the engine first invokes the condition function with the event data. If the condition returns true, the handler executes normally. If false, the handler is skipped silently with no error or retry.

## iii Primitives Used

| Primitive                                                                   | Purpose                                           |
| --------------------------------------------------------------------------- | ------------------------------------------------- |
| `registerFunction(id, handler)` (condition)                                 | Register the condition function (returns boolean) |
| `registerFunction(id, handler)` (handler)                                   | Register the handler function                     |
| `registerTrigger({ type, function_id, config: { condition_function_id } })` | Bind trigger with condition gate                  |

## Reference Implementation

See [../references/trigger-conditions.js](../references/trigger-conditions.js) for the full working example — a condition-gated trigger where a business rule function filters events before the handler processes them.

Also available in **Python**: [../references/trigger-conditions.py](../references/trigger-conditions.py)

Also available in **Rust**: [../references/trigger-conditions.rs](../references/trigger-conditions.rs)

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerFunction('conditions::is-high-value', async (input) => input.new_value?.amount >= 1000)` — condition function
- `registerFunction('orders::notify-high-value', async (input) => { ... })` — handler function
- `registerTrigger({ type: 'state', function_id: 'orders::notify-high-value', config: { scope: 'orders', key: 'status', condition_function_id: 'conditions::is-high-value' } })` — bind with condition
- Condition returns `true` — handler executes
- Condition returns `false` — handler is skipped silently
- Use `conditions::` prefix for condition function IDs to keep them organized

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Replace the condition logic with your business rules (threshold checks, role validation, feature flags)
- Conditions work on all trigger types — use them on HTTP triggers for auth guards, on durable:subscriber triggers for message filtering
- Keep condition functions lightweight and fast since they run on every trigger fire
- Combine multiple business rules in a single condition function rather than chaining conditions
- Condition functions can call `trigger()` internally to check state or other functions

## Pattern Boundaries

- For registering functions and triggers in general, prefer `iii-functions-and-triggers`.
- For state change triggers specifically, prefer `iii-state-reactions`.
- For invocation modes (sync/void/enqueue), prefer `iii-trigger-actions`.
- Stay with `iii-trigger-conditions` when the primary problem is gating trigger execution with a condition check.

## When to Use

- Use this skill when the task is primarily about `iii-trigger-conditions` in the iii engine.
- Use this skill when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
