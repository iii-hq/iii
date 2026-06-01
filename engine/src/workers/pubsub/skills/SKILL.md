---
name: iii-pubsub
description: >-
  Fire-and-forget topic pub/sub: broadcast an event with `publish` and every
  matching `subscribe` trigger receives it. Use for real-time notifications
  where missed events are acceptable.
---

# iii-pubsub

The `iii-pubsub` worker is topic-based publish/subscribe messaging. Publish an event to a named topic with the `publish` function and every registered `subscribe` trigger whose `topic` matches is invoked with the raw payload — no envelope, no persistence, no retries. It is fire-and-forget broadcast: subscribers receive each event as it arrives, and a subscriber that is offline simply misses it.

The worker exposes one callable function (`publish`, registered with the bare function id `"publish"` — no namespace prefix) and one trigger type (`subscribe`). Two adapters: `local` (default; in-memory broadcast channels; only delivers to subscribers in this engine process; no external dependency) and `redis` (`redis_url: ${REDIS_URL:redis://localhost:6379}`; uses Redis Pub/Sub so events propagate across multiple engine instances).

## When to Use

- Real-time notifications consumers may miss without consequence — UI live updates, ephemeral signals, telemetry mirroring.
- Loose coupling where an event source should not know about its consumers.
- Cross-instance fan-out of ephemeral events (configure the `redis` adapter).

## Boundaries

- No persistence, ordering, or retries — a subscriber that is down misses events permanently. Use `iii-queue` (topic mode, `durable:subscriber`) when every consumer must process every event.
- Topic matching is exact string equality; there are no wildcards or hierarchical patterns. Register one trigger per topic.
- An empty topic is rejected: `publish` returns `topic_not_set`, and a `subscribe` trigger registered with an empty topic never fires.
- For key/value or stream change reactivity, use `iii-state` or `iii-stream` instead — `iii-pubsub` carries discrete events, not state.

## Functions

- `publish` — broadcast an event to a topic; every `subscribe` trigger on that exact topic receives the raw `data`. Empty topic returns `topic_not_set`.

## Reactive triggers

Bind a `subscribe` trigger when a function should run every time `publish` broadcasts to a configured topic. The handler receives the raw `data` value from the publish call directly — no envelope. Multiple triggers can subscribe to the same topic; each gets an independent copy (true fan-out).

Reach for it when:

- A domain event from one part of the system should kick off side effects elsewhere without the publisher knowing the consumers.
- You want cross-instance fan-out via the `redis` adapter.

If subscribers must reliably process every event with retries and dead-letter handling, use `iii-queue`'s `durable:subscriber` instead — `subscribe` is fire-and-forget.

### How to bind

1. Register a handler: `iii.registerFunction('notifications::on-order-shipped', handler)`.
2. Register the trigger:

```typescript
iii.registerTrigger({
  type: 'subscribe',
  function_id: 'notifications::on-order-shipped',
  config: {
    topic: 'orders.shipped',  // required, non-empty. Exact match only — no wildcards.
  },
})
```

The handler receives the published `data` unchanged, with no `topic` field; if one handler serves multiple topics, embed the topic inside `data` at publish time. The handler's return value is ignored and there is no retry.

For the published payload shape, call `iii get function info` on `publish` or the handler function id.
