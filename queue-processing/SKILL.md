---
name: queue-processing
description: >-
  Processes async jobs with retries, concurrency control, and ordering guarantees
  using TriggerAction.Enqueue and queue_configs. Use when building durable async
  workflows, background job processing, or any task requiring retry and ordering.
---

# Queue Processing

Comparable to: BullMQ, Celery, SQS

## Key Concepts

- **Enqueue** sends work to a named queue via `TriggerAction.Enqueue({ queue: 'name' })`
- **Standard queues** process up to `concurrency` jobs in parallel, no ordering guarantees
- **FIFO queues** process sequentially within message groups, requires `message_group_field`
- Failed jobs retry with exponential backoff: `backoff_ms * 2^(attempt-1)`
- After `max_retries` exhausted, messages go to a dead-letter queue (DLQ)
- Target functions receive the enqueued payload directly -- no queue-specific handler changes
- Returns a `messageReceiptId` for tracking

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `init(url)` | Connect worker to engine |
| `registerFunction({ id }, handler)` | Define the job processor |
| `trigger({ function_id, payload, action: TriggerAction.Enqueue({ queue }) })` | Enqueue a job |
| `trigger({ function_id, payload })` | Synchronous invocation (for comparison) |
| `trigger({ ..., action: TriggerAction.Void() })` | Fire-and-forget (no durability) |

## Common Patterns

- `TriggerAction.Enqueue({ queue: 'emails' })` -- durable async with retries
- Standard queue for high-throughput parallel work (images, emails, notifications)
- FIFO queue for ordered processing (payments, ledgers, state mutations)
- Queue configs in `iii-config.yaml` under `queue_configs` with `max_retries`, `concurrency`, `type`, `backoff_ms`
- Error handling: catch `enqueue_error` for missing queue or FIFO validation failures

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with config options, queue types, TS/Python/Rust examples, and adapter details.

## Pattern Boundaries

- If the task needs synchronous request-response, use plain `trigger()` without an action.
- If fire-and-forget without durability is fine, use `TriggerAction.Void()`.
- If the task is about scheduled recurring work, use `cron-scheduling`.
- Stay with `queue-processing` when durability, retries, concurrency control, or ordering matter.
