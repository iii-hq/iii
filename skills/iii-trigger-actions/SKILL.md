---
name: iii-trigger-actions
description: >-
  Selects how functions are invoked: synchronous result-returning calls,
  fire-and-forget void dispatches, or durable `TriggerAction.Enqueue({ queue })`
  background jobs. Use whenever a handler should not block the caller, work
  should run later or reliably with retries, a request should return quickly,
  or an agent must choose between inline RPC, optional side effects, and queued
  async processing.
---

# Trigger Actions

Comparable to: RPC vs message queue vs fire-and-forget patterns

## Key Concepts

Use the concepts below when they fit the task. Not every invocation needs all three modes.

- **Synchronous** (default): caller blocks until the function returns a result or times out
- **Void** (`TriggerAction.Void()`): fire-and-forget dispatch, returns immediately, no retry guarantees
- **Enqueue** (`TriggerAction.Enqueue({ queue })`): routes through a named queue with automatic retries and backoff, returns a `messageReceiptId`
- Decision guide: need the result? use sync. Must complete reliably? use enqueue. Optional side effect? use void.

## Architecture

The caller invokes `trigger()` with an optional action parameter. Synchronous mode waits for the handler result. Void mode dispatches and returns immediately. Enqueue mode places the payload on a named queue where a consumer processes it with retry guarantees.

## iii Primitives Used

| Primitive                                                    | Purpose                                        |
| ------------------------------------------------------------ | ---------------------------------------------- |
| `trigger({ function_id, payload })`                          | Synchronous invocation, blocks for result      |
| `trigger({ ..., action: TriggerAction.Void() })`             | Fire-and-forget, returns immediately           |
| `trigger({ ..., action: TriggerAction.Enqueue({ queue }) })` | Durable async via named queue, returns receipt |
| `iii trigger --function-id=ID --payload=JSON`                | CLI trigger (part of the engine binary)        |
| `--timeout-ms`                                               | CLI flag to set trigger timeout (default 30s)  |

## Reference Implementation

See [../references/trigger-actions.js](../references/trigger-actions.js) for the full working example — a comparison of all three

Also available in **Python**: [../references/trigger-actions.py](../references/trigger-actions.py)

Also available in **Rust**: [../references/trigger-actions.rs](../references/trigger-actions.rs)
invocation modes showing when and how to use sync, void, and enqueue patterns.
For a shorter copyable enqueue example, see [iii-examples: Durable Enqueue](../iii-examples/SKILL.md#durable-enqueue).

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `await iii.trigger({ function_id: 'users::get', payload: { id } })` — sync, get result directly
- `iii.trigger({ function_id: 'analytics::track', payload: event, action: TriggerAction.Void() })` — fire-and-forget
- `iii.trigger({ function_id: 'orders::process', payload: order, action: TriggerAction.Enqueue({ queue: 'payments' }) })` — durable enqueue
- Sync returns the function result directly
- Void returns `undefined` in Node/browser, `None` in Python, and `Value::Null` in Rust
- Enqueue returns `{ messageReceiptId: string }` as JSON; Rust may deserialize to `EnqueueResult { message_receipt_id }`
- `iii trigger --function-id='users::get' --payload='{"id":"123"}'` — invoke via CLI
- `iii trigger --function-id='users::get' --payload='{"id":"123"}' --timeout-ms=5000` — with custom timeout

## Timeout and Error Behavior

- The default invocation timeout is 30 seconds unless overridden by SDK init options or per-call `timeoutMs` / `timeout_ms`.
- Sync invocation surfaces handler errors, engine errors, RBAC denial, and timeout errors to the caller.
- Void invocation only confirms dispatch; it does not return handler results and does not give retry guarantees.
- Enqueue confirms message acceptance. Handler failures are retried according to queue configuration and may later land in a dead letter queue.
- Use `iii-error-handling` for `FORBIDDEN`, `TIMEOUT`, `function_not_found`, `function_not_invokable`, `invocation_failed`, and `invocation_stopped`.

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Default to synchronous when the caller needs the result to proceed
- Use void for logging, analytics, or any side effect where failure is acceptable
- Use enqueue for anything that must complete reliably — payments, emails, notifications
- Combine modes in a single handler: sync call for validation, then enqueue for processing
- Named queues let you configure retries and concurrency per workload type

## Pattern Boundaries

- For queue configuration (retries, concurrency, FIFO ordering), prefer `iii-engine-config`.
- For DLQ handling when enqueued jobs exhaust retries, prefer `iii-dead-letter-queues`.
- For error handling, prefer `iii-error-handling`.
- For function registration and trigger binding, prefer `iii-functions-and-triggers`.
- For compact copyable examples across primitives, prefer `iii-examples`.
- Stay with `iii-trigger-actions` when the primary problem is choosing the right invocation mode.

## When to Use

- Use this skill when the task is primarily about `iii-trigger-actions` in the iii engine.
- Use this skill even when the request does not say "enqueue" if work is slow, retryable, reliable, or should continue after the caller returns.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
