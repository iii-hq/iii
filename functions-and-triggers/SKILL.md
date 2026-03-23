---
name: functions-and-triggers
description: >-
  Registers functions and triggers on the iii engine across TypeScript, Python,
  and Rust. Use when creating workers, registering function handlers, binding
  triggers (HTTP, queue, cron, state, stream, subscribe), or invoking functions
  across languages.
---

# Functions & Triggers

The core primitives of any iii system.

## Key Concepts

- A **Function** is an async handler registered with a unique ID
- A **Trigger** binds an event source to a function (HTTP, queue, cron, state, stream, subscribe)
- Functions can invoke other functions via `trigger()` regardless of language or worker location
- The engine handles serialization, routing, and delivery automatically

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `registerFunction({ id }, handler)` | Define a function handler |
| `registerTrigger({ type, function_id, config })` | Bind an event source to a function |
| `trigger({ function_id, payload })` | Invoke a function synchronously |
| `trigger({ ..., action: TriggerAction.Void() })` | Fire-and-forget invocation |
| `trigger({ ..., action: TriggerAction.Enqueue({ queue }) })` | Durable async invocation via queue |

## Common Patterns

- `init('ws://localhost:49134')` — connect to the engine
- `registerFunction({ id: 'namespace::name' }, async (input) => { ... })` — register a handler
- `registerTrigger({ type: 'http', function_id, config: { api_path, http_method } })` — HTTP trigger
- `registerTrigger({ type: 'queue', function_id, config: { topic } })` — queue trigger
- `registerTrigger({ type: 'cron', function_id, config: { expression } })` — cron trigger
- `registerTrigger({ type: 'state', function_id, config: { scope, key } })` — state change trigger
- `registerTrigger({ type: 'subscribe', function_id, config: { topic } })` — pubsub subscriber
- Cross-language invocation: a TypeScript function can trigger a Python or Rust function by ID

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with TypeScript, Python, and Rust examples.

## Pattern Boundaries

- For HTTP endpoint specifics, see `http-endpoints`
- For queue processing details, see `queue-processing`
- For cron scheduling details, see `cron-scheduling`
- For trigger invocation modes (sync/void/enqueue), see `trigger-actions`
