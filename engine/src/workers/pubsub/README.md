# iii-pubsub

Topic-based publish/subscribe messaging for broadcasting events to multiple subscribers in real time.

## Install

```bash
iii worker add iii-pubsub
```

Resolves from the worker registry at [workers.iii.dev](https://workers.iii.dev/).

## Skills

Install the `iii-pubsub` agent skill for Claude Code, Cursor, and 30+ other agents:

```bash
npx skills add iii-hq/iii --full-depth --skill iii-pubsub
```

## Sample Configuration

```yaml
- name: iii-pubsub
  config:
    adapter:
      name: local
```

## Configuration

| Field | Type | Description |
|---|---|---|
| `adapter` | Adapter | Adapter for pub/sub distribution. Defaults to `local` (in-memory). |

## Configure

Runtime settings live in the **`configuration` worker** under id **`iii-pubsub`**. The worker registers its JSON Schema at startup, reads the live value via `configuration::get` (so `${VAR:default}` placeholders in string fields expand against the process env), and hot-applies changes when the value updates.

The config.yaml block above is **seed-only**: it is installed as the initial value on first boot, when no value is stored yet. After that, the configuration worker entry is the source of truth — change the adapter via `configuration::set` or by editing the persisted file (`./config/iii-pubsub.yaml` with the default `fs` adapter); both propagate without an engine restart. Edits to the config.yaml block are ignored once a value is stored.

### Hot Reload

When the `iii-pubsub` configuration changes, the worker re-reads the authoritative value and applies it in place:

- The `adapter` is a **full backend hot-swap**: the new pub/sub backend is built, every live `subscribe` trigger is re-subscribed onto it, the live backend is swapped so new publishes route through it, and the previous backend's subscriptions are torn down (aborting its redis tasks rather than leaking them).
- The swap is **gated**: a value that fails to build the backend is rejected and the previous backend keeps running.
- Invalid values are rejected by schema validation at `configuration::set` time; a stored value that fails to deserialize is logged and the previous config is kept.

Because pub/sub is fire-and-forget, a publish in the brief window mid-swap may be observed by both backends; prefer a quiet moment to repoint the adapter in a multi-instance deployment.

## Adapters

### local

In-memory pub/sub using broadcast channels. Messages are delivered only to subscribers running in the same engine process. No external dependencies required.

```yaml
name: local
```

### redis

Uses Redis Pub/Sub as the backend. Enables event delivery across multiple engine instances.

```yaml
name: redis
config:
  redis_url: ${REDIS_URL:redis://localhost:6379}
```

## Functions

### `publish`

Publish an event to a topic. All functions subscribed to that topic will be invoked with the payload.

| Field | Type | Description |
|---|---|---|
| `topic` | string | Required. The topic to publish to. |
| `data` | any | The event payload to broadcast. |

Returns `null` on success.

## Trigger Type: `subscribe`

Register a function to be invoked whenever an event is published to a topic.

| Config Field | Type | Description |
|---|---|---|
| `topic` | string | Required. The topic to subscribe to. |

The handler receives the raw `data` value passed to the `publish` call directly (no envelope).

### Sample Code

```typescript
const fn = iii.registerFunction(
  { id: 'notifications::onOrderShipped' },
  async (data) => {
    console.log('Order shipped:', data)
    return {}
  },
)

iii.registerTrigger({
  type: 'subscribe',
  function_id: fn.id,
  config: { topic: 'orders.shipped' },
})

await iii.trigger({
  function_id: 'publish',
  payload: {
    topic: 'orders.shipped',
    data: { orderId: 'abc-123', address: '123 Main St' },
  },
  action: TriggerAction.Void(),
})
```

## PubSub vs Queue

| Feature | PubSub | Queue (topic-based) |
|---|---|---|
| Delivery | Broadcast to all subscribers | Fan-out to each subscribed function; replicas compete |
| Persistence | No (fire-and-forget) | Yes (with retries and DLQ) |
| Ordering | Not guaranteed | FIFO within topic |
| Best for | Real-time notifications, fire-and-forget fanout | Reliable fanout with retries and dead-letter support |
