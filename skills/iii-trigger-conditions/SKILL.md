---
name: iii-trigger-conditions
description: >-
  Registers a boolean condition function and attaches it to triggers via
  condition_function_id so handlers only fire when the condition passes. Use when
  gating triggers on business rules, checking user permissions, validating data
  before processing, filtering high-value orders, rate-limiting events, or
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

## Code Examples

TypeScript:

```typescript
iii.registerFunction("conditions::is-high-value", async (event) => {
  return Number(event.new_value?.amount ?? 0) >= 1000;
});

iii.registerFunction("orders::notify-high-value", async (event) => {
  await sendNotification(event.new_value);
  return { notified: true };
});

iii.registerTrigger({
  type: "state",
  function_id: "orders::notify-high-value",
  config: {
    scope: "orders",
    condition_function_id: "conditions::is-high-value",
  },
});
```

Python:

```python
def is_high_value(event):
    return float(event.get("new_value", {}).get("amount", 0)) >= 1000

def notify_high_value(event):
    return {"notified": True, "order": event.get("new_value")}

iii.register_function("conditions::is-high-value", is_high_value)
iii.register_function("orders::notify-high-value", notify_high_value)
iii.register_trigger({
    "type": "state",
    "function_id": "orders::notify-high-value",
    "config": {"scope": "orders", "condition_function_id": "conditions::is-high-value"},
})
```

Rust:

```rust
iii.register_function(RegisterFunction::new("conditions::is-high-value", |event: serde_json::Value| {
    let amount = event["new_value"]["amount"].as_f64().unwrap_or(0.0);
    Ok(serde_json::Value::Bool(amount >= 1000.0))
}))?;

iii.register_trigger(RegisterTriggerInput {
    trigger_type: "state".into(),
    function_id: "orders::notify-high-value".into(),
    config: json!({
        "scope": "orders",
        "condition_function_id": "conditions::is-high-value"
    }),
    metadata: None,
})?;
```

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
- For built-in trigger payload shapes, prefer `iii-trigger-schemas`.
- For invocation modes (sync/void/enqueue), prefer `iii-trigger-actions`.
- Stay with `iii-trigger-conditions` when the primary problem is gating trigger execution with a condition check.

## When to Use

- Use this skill when the task is primarily about `iii-trigger-conditions` in the iii engine.
- Use this skill when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
