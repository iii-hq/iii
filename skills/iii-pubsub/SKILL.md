---
name: iii-pubsub
description: >-
  Broadcast fire-and-forget events through the iii-pubsub worker. Use when
  publishing topic events to multiple subscribers without queue persistence,
  retries, FIFO ordering, or dead-letter handling.
---

# Pub/Sub

`iii-pubsub` is for real-time broadcast inside an iii deployment. It is not the durable queue system.

## Install

```bash
iii worker add iii-pubsub
```

## Core Model

- Producers call `publish` with `{ topic, data }`.
- Subscribers register trigger type `subscribe` with `{ topic }`.
- Subscriber handlers receive the raw `data` value, not an envelope.
- Delivery is best-effort broadcast. Use `iii-queue` for retries, FIFO, and DLQ.

## Example

```typescript
import { registerWorker, TriggerAction } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

iii.registerFunction('notifications::on-order-shipped', async (data) => {
  return { seen: data.orderId }
})

iii.registerTrigger({
  type: 'subscribe',
  function_id: 'notifications::on-order-shipped',
  config: { topic: 'orders.shipped' },
})

await iii.trigger({
  function_id: 'publish',
  payload: {
    topic: 'orders.shipped',
    data: { orderId: 'ord_123' },
  },
  action: TriggerAction.Void(),
})
```

## Configuration

```yaml
workers:
  - name: iii-pubsub
    config:
      adapter:
        name: local
```

Use `adapter.name: redis` with `redis_url` when multiple engine instances need to receive the same topic events.

## Pub/Sub vs Queue

| Need | Use |
| --- | --- |
| Notify every live subscriber now | `iii-pubsub` |
| Retry failed subscribers | `iii-queue` topic mode |
| Direct async function call with retries | `TriggerAction.Enqueue({ queue })` |
| Dead-letter and redrive | `iii-queue` |
| Strict ordering | `iii-queue` FIFO |

## When to Use

- Use this skill when the task mentions pub/sub, broadcasting, topics, non-durable fanout, live notifications, or fire-and-forget event distribution.

## Boundaries

- Do not use pub/sub for guaranteed delivery.
- Do not use `iii::durable::publish` when the request explicitly needs non-durable pub/sub.
- Do not generate removed service APIs or stream adapter SDK APIs.
