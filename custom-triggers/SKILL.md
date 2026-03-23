---
name: custom-triggers
description: >-
  Build custom trigger types for events iii does not handle natively. Use when
  integrating webhooks, file watchers, IoT devices, database CDC, polling
  services, or any external event source not covered by built-in trigger types.
---

# Custom Triggers

Extend the iii engine with new trigger types for any event source.

## Key Concepts

- A **Custom Trigger Type** is a handler you register with the engine to listen for events it does not natively support
- The handler implements two callbacks: `registerTrigger` (start listening) and `unregisterTrigger` (stop and clean up)
- Both callbacks receive a `TriggerConfig` with `id`, `function_id`, and `config`
- Once registered, functions bind to your trigger type the same way they bind to built-in types
- Built-in trigger types: `http`, `cron`, `queue`, `state`, `stream`, `subscribe`

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `registerTriggerType({ id, description }, handler)` | Register a custom trigger type with the engine |
| `unregisterTriggerType({ id, description })` | Remove a custom trigger type |
| `registerTrigger({ type, function_id, config })` | Bind a function to a trigger type (built-in or custom) |
| `trigger({ function_id, payload })` | Invoke the bound function when your event fires |

## Common Patterns

- `registerTriggerType({ id: 'webhook', description: '...' }, handler)` -- register a new trigger type
- Handler `registerTrigger(config)` -- called when a function binds; set up listeners, subscriptions, or pollers
- Handler `unregisterTrigger(config)` -- called when binding is removed; tear down resources
- `config.function_id` -- the target function to invoke when the event fires
- `config.config` -- caller-supplied configuration (e.g., path, interval, topic)
- `trigger({ function_id: config.function_id, payload })` -- dispatch the event to the bound function
- `unregisterTriggerType({ id: 'webhook', description: '...' })` -- remove the trigger type entirely

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with TypeScript, Python, and Rust examples.

## Pattern Boundaries

- For binding functions to built-in trigger types, see `functions-and-triggers`
- For HTTP endpoint triggers specifically, see `http-endpoints`
- For queue-based triggers, see `queue-processing`
- For cron scheduling triggers, see `cron-scheduling`
