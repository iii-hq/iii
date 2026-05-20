---
type: how-to
trigger_type: subscribe
title: React to topic publishes
---

# When to use

Register a `subscribe` trigger when a function should run automatically every time `publish` broadcasts to a configured topic. The handler receives the raw `data` value from the publish call — no envelope. Multiple triggers can subscribe to the same topic; each receives an independent copy of every event.

Reach for it when:

- A domain event from one part of the system should kick off side effects elsewhere — and the publisher shouldn't have to know about the consumers.
- You want loose coupling between an event source and its handlers (audit logs, mirror writes, real-time notifications).
- You're bridging cross-instance fan-out (configure the worker's `redis` adapter so events propagate across an engine fleet).

Use a state or stream reactive trigger instead when the source of truth is a key/value store change rather than a discrete event. Use the `iii-queue` worker's `durable:subscriber` trigger when subscribers must process every event reliably (with retries and dead-letter handling); `subscribe` is fire-and-forget — a subscriber that's offline misses events.

Prerequisite: the `iii-pubsub` worker must be enabled in `config.yaml`. Handlers and triggers are registered from a connected worker via `iii.registerFunction` and `iii.registerTrigger`.

# Inputs

Registration is the standard two-step pattern: define the handler, then bind it to the `subscribe` trigger.

## Handler function

Register any function id. The handler receives the raw `data` value passed to the matching `publish` call — no envelope, no metadata, just the payload.

```json
// iii.registerFunction — handler id only; no engine payload.
{ "id": "notifications::on-order-shipped" }
```

## Trigger registration

```json
{
  "type":        "subscribe",                           // required. Must be exactly "subscribe".
  "function_id": "notifications::on-order-shipped",     // required. Handler invoked when a publish matches the topic.
  "config": {
    "topic": "orders.shipped"                           // required (non-empty). The trigger only fires for publishes to this exact topic — no wildcards.
  }
}
```

`type`, `function_id`, and `config.topic` are required. `topic` must be non-empty — registering with an empty topic logs a warning and the trigger never fires (per `engine/src/workers/pubsub/pubsub.rs:101-105`). Topic matching is **exact string equality**; the worker does not interpret hierarchical patterns or wildcards.

To subscribe to multiple topics, register one trigger per topic. Multiple triggers on the same topic each fire independently — the broadcast is true fan-out.

# Outputs

The handler receives the raw `data` value the publisher sent — whatever JSON shape that is, returned unchanged:

```json
// Example: when the publisher sent `data: { "orderId": "abc-123", "status": "shipped" }`
{ "orderId": "abc-123", "status": "shipped" }
```

There is no envelope, no `topic` field, no metadata. If the handler needs to know which topic the message arrived on (e.g., one handler bound to multiple topics), include the topic inside the published `data` itself.

The handler's return value is **ignored**. Errors from the handler are logged but do not affect the originating `publish` call site or other subscribers — each subscriber runs independently. There is no retry: a handler that fails skips the event entirely.

# Worked example

Subscribe a handler to the `orders.shipped` topic so it fires on every matching `publish`:

```json
{
  "type":        "subscribe",
  "function_id": "notifications::on-order-shipped",
  "config":      { "topic": "orders.shipped" }
}
```

The typical pattern is one trigger per (handler, topic) pair: a notifications handler subscribed to `orders.shipped`, an audit handler subscribed to `users.deleted`, etc. For multi-topic handlers, embed the topic in the `data` payload at publish time. For runnable scaffolds, see the pubsub worker source and SDK examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [`publish`](iii://iii-pubsub/pubsub/publish) — the broadcast side. Every event delivered to this trigger comes from a `publish` call.
- `iii-pubsub` adapter config (see [the README](../../../README.md)) — `local` adapter only delivers to subscribers on the same engine instance; use `redis` for cross-instance fan-out.
- `iii-queue` topic-based subscribers (`durable:subscriber` trigger) — for reliable delivery with retries and dead-letter support; `subscribe` is fire-and-forget.
