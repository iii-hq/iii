---
name: iii-queue-processing
description: >-
  Enqueues jobs, configures retry policies, sets concurrency limits, and orders
  messages via named standard or FIFO queues. Use when building background job
  workers, task queues, message queues, async pipelines, or any pattern needing
  guaranteed delivery with exponential backoff and dead-letter handling.
---

# Queue Processing

Comparable to: BullMQ, Celery, SQS

## Key Concepts

Use the concepts below when they fit the task. Not every queue setup needs all of them.

- **Named queues** are declared in `iii-config.yaml` under `queue_configs`
- **Standard queues** process jobs concurrently; **FIFO queues** preserve ordering
- `TriggerAction.Enqueue({ queue })` dispatches a job to a named queue
- Failed jobs **auto-retry** with exponential backoff up to `max_retries`
- Jobs that exhaust retries land in a **dead letter queue** for inspection
- Each consumer function receives the job payload and a `messageReceiptId`
- **Fan-out** distributes jobs to all worker instances registered for a function, not just one

## Architecture

    Producer function
      → TriggerAction.Enqueue({ queue: 'task-queue' })
        → Named Queue (standard or FIFO)
          → Consumer registerFunction handler
            → success / retry with backoff
              → Dead Letter Queue (after max_retries)

## iii Primitives Used

| Primitive                                                    | Purpose                                        |
| ------------------------------------------------------------ | ---------------------------------------------- |
| `registerFunction`                                           | Define the consumer that processes jobs        |
| `trigger({ ..., action: TriggerAction.Enqueue({ queue }) })` | Dispatch a job to a named queue                |
| `messageReceiptId`                                           | Acknowledge or track individual job processing |
| `queue_configs` in `iii-config.yaml`                         | Declare queues with concurrency and retries    |

## Reference Implementation

See [../references/queue-processing.js](../references/queue-processing.js) for the full working example — a producer that enqueues jobs and a consumer that processes them with retry logic.

Also available in **Python**: [../references/queue-processing.py](../references/queue-processing.py)

Also available in **Rust**: [../references/queue-processing.rs](../references/queue-processing.rs)

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `registerFunction({ id }, handler)` — define the consumer
- `trigger({ function_id, payload, action: TriggerAction.Enqueue({ queue }) })` — enqueue a job
- `payload.messageReceiptId` — track or acknowledge the job
- `trigger({ function_id: 'state::set', payload })` — persist results after processing
- `const logger = new Logger()` — structured logging per job

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Choose FIFO queues when job ordering matters (e.g. sequential pipeline steps)
- Set `max_retries` and `concurrency` in queue config to match your workload
- Chain multiple queues for multi-stage pipelines (queue A consumer enqueues to queue B)
- For idempotency, check state before processing to avoid duplicate work on retries
- Use fan-out queues when every worker instance must receive each job (e.g. cache invalidation, config reload)

## Engine Configuration

Named queues are declared in iii-config.yaml under `queue_configs` with per-queue `max_retries`, `concurrency`, `type`, `backoff_ms`, and `fanout` (boolean). When `fanout: true`, every worker instance registered for the consumer function receives the job. See [../references/iii-config.yaml](../references/iii-config.yaml) for the full annotated config reference.

## Pattern Boundaries

- If the task only needs fire-and-forget without retries or ordering, prefer `iii-trigger-actions` with `TriggerAction.Void()`.
- If failed jobs need special handling or alerting, prefer `iii-dead-letter-queues` for the DLQ consumer.
- If the task is step-by-step orchestration with branching, prefer `iii-workflow-orchestration`.
- Stay with `iii-queue-processing` when the primary need is reliable async job execution with retries.

## When to Use

- Use this skill when the task is primarily about `iii-queue-processing` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
