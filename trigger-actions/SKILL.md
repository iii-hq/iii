---
name: trigger-actions
description: >-
  Invoke iii functions in three modes: synchronous (block for result),
  fire-and-forget (void, no wait), or enqueue (durable with retries). Use when
  deciding how to call a function based on reliability, latency, and result
  requirements.
---

# Trigger Actions

Three invocation modes for calling functions.

## Key Concepts

- **Synchronous** (default): caller blocks until the function returns or times out
- **Void**: fire-and-forget dispatch with no delivery guarantees and no return value
- **Enqueue**: routes through a named queue with automatic retries, backoff, and dead-letter handling
- Decision: need the result? use sync. Must complete reliably? use enqueue. Optional side effect? use void.

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `trigger({ function_id, payload })` | Synchronous invocation, blocks for result |
| `trigger({ ..., action: TriggerAction.Void() })` | Fire-and-forget, returns immediately |
| `trigger({ ..., action: TriggerAction.Enqueue({ queue }) })` | Durable async via named queue |

## Common Patterns

- `await iii.trigger({ function_id: 'users::get', payload: { id } })` -- sync, get result
- `iii.trigger({ function_id: 'analytics::track', payload: event, action: TriggerAction.Void() })` -- fire-and-forget
- `iii.trigger({ function_id: 'orders::process', payload: order, action: TriggerAction.Enqueue({ queue: 'payments' }) })` -- durable enqueue
- Sync returns the function result directly
- Void returns `null` / `None`
- Enqueue returns `{ messageReceiptId: string }` for tracking

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with comparison matrix and TypeScript/Python/Rust examples.

## Pattern Boundaries

- For function registration and trigger binding, see `functions-and-triggers`
- For queue configuration and processing details, see `queue-processing`
- For dead letter queue inspection and redriving, see `dead-letter-queues`
