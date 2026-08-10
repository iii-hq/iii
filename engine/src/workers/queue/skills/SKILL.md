---
name: iii-queue
description: >-
  Asynchronous job processing with named queues, retries, FIFO ordering, and
  dead-letter support, plus durable topic pub/sub. Reach for it when every
  consumer must reliably process every message.
---

# iii-queue

The `iii-queue` worker provides asynchronous job processing with retries, configurable concurrency, FIFO ordering, and dead-letter (DLQ) support. It runs in two modes. Topic-based queues are durable pub/sub: register a consumer with a `durable:subscriber` trigger and produce with `iii::durable::publish` — every distinct function subscribed to a topic receives a copy of each message (fan-out). Named queues are defined in config (`queue_configs`) and targeted by enqueuing a function call with `TriggerAction.Enqueue` — no trigger registration needed, the target function is the consumer.

Three adapters back delivery: `builtin` (in-process; `in_memory` or `file_based`; single-instance; retries + DLQ + FIFO), `rabbitmq` (durable delivery, retries, and DLQ across instances), and `redis` (multi-instance topic pub/sub only — it publishes to named queues but does not implement named-queue consumption, retries, or DLQ). Choose `builtin` for local and single-instance, `rabbitmq` for multi-instance production.

## When to Use

- A unit of work should run asynchronously with automatic retries and dead-letter capture on repeated failure.
- You need strict ordering within a key (FIFO) — payments, ledger entries, state machines — via a `fifo` queue with `message_group_field`.
- Durable fan-out where every subscribed consumer must process every event (unlike `iii-pubsub`, which is fire-and-forget).
- You need to inspect or recover stuck work: browse a DLQ and redrive or discard messages.

## Boundaries

- Not fire-and-forget broadcast — for ephemeral notifications where missed events are fine, use `iii-pubsub`.
- The `redis` adapter is publish-only for named queues; named-queue consumption, retries, and DLQ require `builtin` or `rabbitmq`.
- FIFO queues force `prefetch=1` (one job at a time) and require `message_group_field` present and non-null in every payload; they trade throughput for ordering and retry inline (blocking the group until success or DLQ).
- This worker carries work items, not key/value or stream state — use `iii-state` / `iii-stream` for those.

## Functions

- `iii::durable::publish` — publish a message to a topic; fanned out to every distinct subscribed function. Empty topic returns `topic_not_set`.
- `iii::queue::redrive` — move all messages from a named queue's DLQ back to the main queue.
- `iii::queue::redrive_message` — move a single DLQ message back to the main queue by id.
- `iii::queue::discard_message` — purge a single DLQ message by id.
- `engine::queue::list_topics`, `engine::queue::topic_stats`, `engine::queue::dlq_topics`, `engine::queue::dlq_messages` — console/inspection helpers: enumerate topics, read per-topic stats, list DLQ topics with counts, and browse DLQ messages.

## Reactive triggers

Bind a `durable:subscriber` trigger when a function should durably consume every message published to a topic, with retries and dead-letter handling. Distinct functions on the same topic each receive every message (fan-out); replicas of the same function compete for messages.

Reach for it when:

- A topic event must be processed reliably — a consumer that was offline should still get the message once it returns.
- You want retries and a DLQ around the consumer rather than the best-effort delivery of `iii-pubsub`.

For a single target function with retries (not fan-out), skip the trigger and enqueue the call directly with `TriggerAction.Enqueue({ queue })` against a queue defined in `queue_configs`.

### How to bind

1. Register a handler: `iii.registerFunction('orders::process', handler)`.
2. Register the trigger:

```typescript
iii.registerTrigger({
  type: 'durable:subscriber',
  function_id: 'orders::process',
  config: {
    topic: 'orders.created',  // topic to consume.
    // optional per-trigger queue_config (max_retries, concurrency, type, ...).
  },
})
```

Per-queue tuning (in `queue_configs` or `queue_config`): `max_retries` (default `3`), `concurrency` (default `10`; FIFO forces `1`), `type` (`standard` | `fifo`), `message_group_field` (required for `fifo`), `backoff_ms` (default `1000`, exponential), `poll_interval_ms` (default `100`), `dispatch_timeout_ms` (optional; nacks a dispatched-but-uncompleted job back through retry→DLQ instead of leaving it in-flight until the broker timeout — does not cancel worker-side work, so bounded handlers must be idempotent).

For the message payload shape, call `iii get function info` on the trigger type or handler function id.
