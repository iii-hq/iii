---
name: queue-processing
description: >-
  Processes async jobs with retries, concurrency, and ordering via named queues.
  Use when offloading work, building pipelines, or needing guaranteed delivery.
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

## Architecture

    Producer function
      → TriggerAction.Enqueue({ queue: 'task-queue' })
        → Named Queue (standard or FIFO)
          → Consumer registerFunction handler
            → success / retry with backoff
              → Dead Letter Queue (after max_retries)

## iii Primitives Used

| Primitive                                                       | Purpose                                       |
| --------------------------------------------------------------- | --------------------------------------------- |
| `registerFunction`                                              | Define the consumer that processes jobs        |
| `trigger({ ..., action: TriggerAction.Enqueue({ queue }) })`    | Dispatch a job to a named queue                |
| `messageReceiptId`                                              | Acknowledge or track individual job processing |
| `queue_configs` in `iii-config.yaml`                             | Declare queues with concurrency and retries    |

## Reference Implementation

See [../references/queue-processing.js](../references/queue-processing.js) for the full working example — a producer that enqueues jobs and a consumer that processes them with retry logic.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `registerFunction(id, handler)` — define the consumer
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

## Pattern Boundaries

- If the task only needs fire-and-forget without retries or ordering, prefer `trigger-actions` with `TriggerAction.Void()`.
- If failed jobs need special handling or alerting, prefer `dead-letter-queues` for the DLQ consumer.
- If the task is step-by-step orchestration with branching, prefer `workflow-orchestration`.
- Stay with `queue-processing` when the primary need is reliable async job execution with retries.
