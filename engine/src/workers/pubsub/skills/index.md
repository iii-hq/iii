---
type: index
title: iii-pubsub
---

# iii-pubsub

Topic-based publish/subscribe messaging. Publish an event to a named topic with the `publish` engine function and every registered `subscribe` trigger whose `topic` matches gets invoked with the raw payload — no envelope, no persistence, no retries. The worker is fire-and-forget broadcast: subscribers receive each event as it arrives, missed events are gone.

The worker exposes **one callable function** (`publish`, no namespace prefix — the function id is literally the string `"publish"`) and **one trigger type** (`subscribe`). Two adapters: `local` (in-memory, single engine instance) and `redis` (Redis pub/sub, multi-instance fan-out). For the adapter config block, see [the README](../README.md).

This worker is the right tool for **real-time notifications** that consumers may miss without consequence (UI live updates, ephemeral signals, telemetry mirroring). For reliable fan-out with retries and dead-letter handling, use the `iii-queue` worker's topic-based queue mode instead — see the comparison in [the README's "PubSub vs Queue" section](../README.md).

- **`publish` function** — broadcast an event to a topic. Empty topic returns `topic_not_set`.
- **`subscribe` trigger** — register a function to fire on every publish to a configured topic.

## How-tos

### `publish`

- [`publish`](iii://iii-pubsub/pubsub/publish) — broadcast an event to a topic so every registered subscriber receives the payload.

### `subscribe` triggers

- [React to topic publishes](iii://iii-pubsub/pubsub/reactive-triggers) — register a `subscribe` trigger to invoke a handler on every publish to a configured topic; the handler receives the raw `data` value directly.
