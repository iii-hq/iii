---
name: dead-letter-queues
description: >-
  Inspect and redrive jobs that exhausted all retries. Use when queue jobs fail
  permanently, need investigation, and must be retried after fixing the root
  cause. Works with RabbitMQ management UI, CLI, and the iii::queue::redrive
  built-in function.
---

# Dead Letter Queues

Inspect failed jobs and redrive them after fixing the root cause.

## Key Concepts

- Jobs move to a **DLQ** after exhausting `max_retries` attempts with exponential backoff (`backoff_ms * 2^attempt`)
- Each DLQ entry preserves the original payload, last error, timestamp, and job metadata
- Inspect via RabbitMQ Management UI or `rabbitmqadmin` CLI
- Redrive via the built-in `iii::queue::redrive` function or the `iii` CLI
- Redriving resets attempt counters to zero, giving jobs a fresh retry cycle
- Always investigate and deploy fixes before redriving

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `trigger({ function_id: 'iii::queue::redrive', payload: { queue } })` | Redrive all DLQ jobs for a queue |
| `iii trigger --function-id='iii::queue::redrive' --payload='{"queue":"name"}'` | CLI redrive |

## Common Patterns

- Redrive via SDK: `await iii.trigger({ function_id: 'iii::queue::redrive', payload: { queue: 'payment' } })`
- Redrive via CLI: `iii trigger --function-id='iii::queue::redrive' --payload='{"queue": "payment"}'`
- Returns: `{ queue: 'payment', redriven: 12 }`
- Inspect in RabbitMQ UI: `http://localhost:15672`, find `iii.__fn_queue::{name}::dlq.queue`
- Inspect via CLI: `rabbitmqadmin get queue="iii.__fn_queue::payment::dlq.queue" count=10 ackmode=ack_requeue_true`
- Best practice: investigate failures, deploy fix, then redrive

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with RabbitMQ topology, inspection methods, and best practices.

## Pattern Boundaries

- For queue configuration and processing, see `queue-processing`
- For trigger invocation modes (sync/void/enqueue), see `trigger-actions`
- For function registration, see `functions-and-triggers`
