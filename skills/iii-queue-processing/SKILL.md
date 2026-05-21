---
name: iii-queue-processing
description: >-
  Uses `TriggerAction.Enqueue({ queue })` and named queues for reliable
  background work. Use when a request should hand off slow work and return
  quickly, a task must retry on failure, jobs need concurrency limits or FIFO
  ordering, or a workflow needs durable async processing, backoff, and
  dead-letter handling.
---

# Queue Processing

Comparable to: BullMQ, Celery, SQS

## Key Concepts

Use the concepts below when they fit the task. Not every queue setup needs all of them.

- **Named queues** are declared in `iii-config.yaml` under `queue_configs`
- **Standard queues** process jobs concurrently; **FIFO queues** preserve ordering
- `TriggerAction.Enqueue({ queue })` dispatches a job to a named queue
- Install or enable queues with `iii worker add iii-queue`
- Topic-based durable fanout uses `iii::durable::publish` plus trigger type `durable:subscriber`
- Failed jobs **auto-retry** with exponential backoff up to `max_retries`
- Jobs that exhaust retries land in a **dead letter queue** for inspection
- Each consumer function receives the job payload and a `messageReceiptId`
- **Fan-out** is achieved by having one producer trigger multiple distinct consumer functions via separate enqueue calls

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
| `trigger({ function_id: 'iii::durable::publish', payload })` | Publish a durable topic event to subscribers   |
| `registerTrigger({ type: 'durable:subscriber' })`            | Subscribe a function to a durable topic        |
| `messageReceiptId`                                           | Acknowledge or track individual job processing |
| `queue_configs` in `iii-config.yaml`                         | Declare queues with concurrency and retries    |

## Reference Implementation

See [../references/queue-processing.js](../references/queue-processing.js) for the full working example — a producer that enqueues jobs and a consumer that processes them with retry logic.

Also available in **Python**: [../references/queue-processing.py](../references/queue-processing.py)

Also available in **Rust**: [../references/queue-processing.rs](../references/queue-processing.rs)

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `registerFunction(id, handler)` — define the consumer
- `trigger({ function_id, payload, action: TriggerAction.Enqueue({ queue }) })` — enqueue a job
- `trigger({ function_id: 'iii::durable::publish', payload: { topic, data } })` — durable fanout event
- `registerTrigger({ type: 'durable:subscriber', config: { topic } })` — durable topic consumer
- `payload.messageReceiptId` — track or acknowledge the job
- `trigger({ function_id: 'state::set', payload })` — persist results after processing
- `const logger = new Logger()` — structured logging per job

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Choose FIFO queues when job ordering matters (e.g. sequential pipeline steps)
- Set `max_retries` and `concurrency` in queue config to match your workload
- Chain multiple queues for multi-stage pipelines (queue A consumer enqueues to queue B)
- For idempotency, check state before processing to avoid duplicate work on retries
- Use fan-out by enqueuing to multiple consumer functions when a single event requires parallel processing (e.g. payment + notification + audit)
- Use topic-based durable queues when many functions should receive the same event with retry/DLQ semantics.

## Engine Configuration

Named queues are declared in iii-config.yaml under `queue_configs` with per-queue `max_retries`, `concurrency`, `type`, and `backoff_ms`. Fan-out is a pattern (one producer triggers multiple consumer functions), not a queue config key. See [../references/iii-config.yaml](../references/iii-config.yaml) for the full annotated config reference.

Install/enable the queue worker with `iii worker add iii-queue`. Use the `builtin` adapter for local/single-instance deployments and RabbitMQ for multi-instance named queues with retries, FIFO, and DLQ. Redis is for topic-style pub/sub and publish-only named queue paths, not full named queue consumption.

## Pattern Boundaries

- If the task only needs fire-and-forget without retries or ordering, prefer `iii-trigger-actions` with `TriggerAction.Void()`.
- If failed jobs need special handling or alerting, prefer `iii-dead-letter-queues` for the DLQ consumer.
- If the task is step-by-step orchestration with branching, prefer `iii-workflow-orchestration`.
- Stay with `iii-queue-processing` when the primary need is reliable async job execution with retries.

## When to Use

- Use this skill when the task is primarily about `iii-queue-processing` in the iii engine.
- Use this skill even when the request does not say "queue" or "enqueue" if the work is slow, retryable, durable, ordered, or should not block an HTTP/UI caller.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
