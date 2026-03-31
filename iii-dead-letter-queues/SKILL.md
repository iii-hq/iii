---
name: iii-dead-letter-queues
description: >-
  Inspects and redrives jobs that exhausted all retries. Use when handling
  failed queue jobs, debugging processing errors, or implementing retry
  strategies.
---

# Dead Letter Queues

Comparable to: SQS DLQ, RabbitMQ dead-letter exchanges

## Key Concepts

Use the concepts below when they fit the task. Not every queue failure needs manual DLQ intervention.

- Jobs move to a **DLQ** after exhausting `max_retries` with exponential backoff (`backoff_ms * 2^attempt`)
- Each DLQ entry preserves the original payload, last error, timestamp, and job metadata
- **Redrive** via the built-in `iii::queue::redrive` function or the `iii trigger` CLI command
- Redriving resets attempt counters to zero, giving jobs a fresh retry cycle
- Always investigate and deploy fixes before redriving — blindly redriving repeats failures
- DLQ support available on Builtin and RabbitMQ adapters

## Architecture

A queue consumer fails processing a job. The engine retries with exponential backoff up to `max_retries`. Once exhausted, the message moves to the DLQ. An operator inspects the failure, deploys a fix, then redrives the DLQ to replay all failed jobs.

## iii Primitives Used

| Primitive                                                                      | Purpose                                  |
| ------------------------------------------------------------------------------ | ---------------------------------------- |
| `trigger({ function_id: 'iii::queue::redrive', payload: { queue } })`          | Redrive all DLQ jobs for a named queue   |
| `trigger({ function_id: 'iii::queue::status', payload: { queue } })`           | Check queue and DLQ status               |
| `iii trigger --function-id='iii::queue::redrive' --payload='{"queue":"name"}'` | CLI redrive command (part of the engine binary) |
| `--timeout-ms`                                                                 | CLI flag to set trigger timeout (default 30s)   |
| `queue_configs` in iii-config.yaml                                             | Configure `max_retries` and `backoff_ms` |

## Reference Implementation

See [../references/dead-letter-queues.js](../references/dead-letter-queues.js) for the full working example — inspecting DLQ status,

Also available in **Python**: [../references/dead-letter-queues.py](../references/dead-letter-queues.py)

Also available in **Rust**: [../references/dead-letter-queues.rs](../references/dead-letter-queues.rs)
redriving failed jobs via SDK and CLI, and configuring retry behavior.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `await iii.trigger({ function_id: 'iii::queue::redrive', payload: { queue: 'payment' } })` — redrive via SDK
- `iii trigger --function-id='iii::queue::redrive' --payload='{"queue": "payment"}'` — redrive via CLI
- `iii trigger --function-id='iii::queue::redrive' --payload='{"queue": "payment"}' --timeout-ms=60000` — with custom timeout
- Redrive returns `{ queue: 'payment', redriven: 12 }` indicating count of replayed jobs
- Inspect in RabbitMQ UI at `http://localhost:15672`, find `iii.__fn_queue::{name}::dlq.queue`
- Best practice: investigate failures, deploy fix, then redrive
- Monitor DLQ depth as an operational alert signal

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Set `max_retries` and `backoff_ms` in `queue_configs` based on your failure tolerance
- Build an admin endpoint that calls `iii::queue::redrive` for operational control
- Use `iii::queue::status` to check DLQ depth before and after redriving
- For dev/test, use lower retry counts to surface failures faster
- In production with RabbitMQ, use the management UI for detailed message inspection
- Consider building an alerting function that triggers on DLQ depth thresholds

## Engine Configuration

Queue `max_retries` and `backoff_ms` are set per queue in iii-config.yaml under `queue_configs`. See [../references/iii-config.yaml](../references/iii-config.yaml) for the full annotated config reference.

## Pattern Boundaries

- For queue processing patterns (enqueue, concurrency, FIFO), prefer `iii-queue-processing`.
- For queue configuration (retries, backoff, adapters), prefer `iii-engine-config`.
- For function registration and triggers, prefer `iii-functions-and-triggers`.
- Stay with `iii-dead-letter-queues` when the primary problem is inspecting or redriving failed jobs.

## When to Use

- Use this skill when the task is primarily about `iii-dead-letter-queues` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
