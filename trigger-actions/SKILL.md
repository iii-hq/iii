---
name: trigger-actions
description: >-
  Selects how functions are invoked — synchronous calls that return results,
  fire-and-forget void dispatches, or durable enqueue through named queues with
  retries. Use when deciding between blocking RPC calls, background job
  dispatch, async workers, or reliable message delivery with acknowledgement.
---

# Trigger Actions

Comparable to: RPC vs message queue vs fire-and-forget patterns

## Key Concepts

Use the concepts below when they fit the task. Not every invocation needs all three modes.

- **Synchronous** (default): caller blocks until the function returns a result or times out
- **Void** (`TriggerAction.Void()`): fire-and-forget dispatch, returns immediately with `null`, no retry guarantees
- **Enqueue** (`TriggerAction.Enqueue({ queue })`): routes through a named queue with automatic retries and backoff, returns a `messageReceiptId`
- Decision guide: need the result? use sync. Must complete reliably? use enqueue. Optional side effect? use void.

## Architecture

The caller invokes `trigger()` with an optional action parameter. Synchronous mode waits for the handler result. Void mode dispatches and returns null immediately. Enqueue mode places the payload on a named queue where a consumer processes it with retry guarantees.

## iii Primitives Used

| Primitive                                                    | Purpose                                          |
| ------------------------------------------------------------ | ------------------------------------------------ |
| `trigger({ function_id, payload })`                          | Synchronous invocation, blocks for result        |
| `trigger({ ..., action: TriggerAction.Void() })`             | Fire-and-forget, returns immediately with null   |
| `trigger({ ..., action: TriggerAction.Enqueue({ queue }) })` | Durable async via named queue, returns receipt   |

## Reference Implementation

See [../references/trigger-actions.js](../references/trigger-actions.js) for the full working example — a comparison of all three
invocation modes showing when and how to use sync, void, and enqueue patterns.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `await iii.trigger({ function_id: 'users::get', payload: { id } })` — sync, get result directly
- `iii.trigger({ function_id: 'analytics::track', payload: event, action: TriggerAction.Void() })` — fire-and-forget
- `iii.trigger({ function_id: 'orders::process', payload: order, action: TriggerAction.Enqueue({ queue: 'payments' }) })` — durable enqueue
- Sync returns the function result directly
- Void returns `null` / `None`
- Enqueue returns `{ messageReceiptId: string }` for tracking

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Default to synchronous when the caller needs the result to proceed
- Use void for logging, analytics, or any side effect where failure is acceptable
- Use enqueue for anything that must complete reliably — payments, emails, notifications
- Combine modes in a single handler: sync call for validation, then enqueue for processing
- Named queues let you configure retries and concurrency per workload type

## Pattern Boundaries

- For queue configuration (retries, concurrency, FIFO ordering), prefer `engine-config`.
- For DLQ handling when enqueued jobs exhaust retries, prefer `dead-letter-queues`.
- For function registration and trigger binding, prefer `functions-and-triggers`.
- Stay with `trigger-actions` when the primary problem is choosing the right invocation mode.
