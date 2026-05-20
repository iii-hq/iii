---
type: how-to
function_id: publish
title: Broadcast an event to every subscriber of a topic
---

# When to use

Call `publish` to broadcast a payload to every function currently subscribed to a topic. The call is fire-and-forget — `publish` returns as soon as the adapter has accepted the event; subscribers receive it asynchronously. There is no persistence, no retry, no acknowledgement.

Reach for it when:

- A single source of truth (a webhook handler, a state writer, a domain event emitter) needs to fan out a notification to multiple unrelated consumers (UI updates, audit logs, secondary projections).
- The consumers are independent and additive — adding a new subscriber should not require changing the publisher.
- The information is real-time — late subscribers do not need to see past events.

Use the `iii-queue` worker's topic-based queue mode instead when delivery must survive a subscriber being offline, or when retries / dead-letter behavior matter — `publish` will drop messages that no subscriber consumes.

Note: `publish` is registered with the **bare function id `"publish"`** — there is no namespace prefix. Call it with `function_id: "publish"`.

# Inputs

```json
{
  "topic": "orders.shipped",                          // required. Non-empty topic name. Empty topic returns a `topic_not_set` error.
  "data":  { "orderId": "abc-123", "status": "shipped" }   // required. Arbitrary JSON payload delivered to each subscriber as-is.
}
```

`topic` and `data` are required. `topic` must be non-empty (the worker validates this at the handler boundary and returns a `topic_not_set` failure code otherwise).

Topic names are opaque strings — the worker does not interpret hierarchy, wildcards, or any other structure. A subscriber registered for `orders.shipped` only matches that exact string; `orders.*` is not a wildcard pattern.

# Outputs

`publish` returns `null` on success — the response carries no useful body. Subscribers run asynchronously after the publisher's call returns.

```json
null
```

When the call fails it returns a `FunctionResult::Failure` with one of:

| `code`            | When                                                                         |
|-------------------|------------------------------------------------------------------------------|
| `"topic_not_set"` | `topic` was empty or whitespace.                                              |
| Adapter-specific  | The configured adapter (`local` or `redis`) refused the publish — for `redis`, this surfaces transport errors with the underlying message. |

# Worked example

Broadcast that an order shipped — every function subscribed to `orders.shipped` receives the payload:

```json
{
  "topic": "orders.shipped",
  "data":  { "orderId": "abc-123", "status": "shipped" }
}
```

The typical pattern is one publish call per domain event the rest of the system should notice. Pick stable topic names that describe the event (`orders.shipped`, `users.deleted`, `auth.login_failed`) and let unrelated subscribers attach as needed. For runnable scaffolds, see the pubsub worker source and SDK examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [React to topic publishes](iii://iii-pubsub/pubsub/reactive-triggers) — the matching subscribe side; handlers receive the `data` payload directly.
- `iii-pubsub` adapter config (see [the README](../../../README.md)) — `local` is single-instance only; multi-instance fleets need `redis`.
- `iii-queue` topic mode — when delivery must survive offline subscribers, retries, or dead-letter handling. See the README's "PubSub vs Queue" comparison.
