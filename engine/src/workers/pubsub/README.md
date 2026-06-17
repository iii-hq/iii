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

## Runtime configuration (hot reload)

`iii-pubsub` registers its configuration with the builtin `configuration` worker
under the id **`iii-pubsub`**, so the `adapter` can be read and changed at runtime
(e.g. `configuration::set { id: "iii-pubsub", value: { adapter: { name: "redis",
config: { redis_url: "..." } } } }`) without restarting the engine. The
config.yaml block is the **seed** used on first boot only; afterwards the
configuration entry is the runtime source of truth and a runtime edit survives
engine restarts. Values are validated against the schema at set time, and
`${VAR:default}` placeholders are expanded on read.

The `adapter` applies as a **full backend hot-swap**: the new pub/sub backend is
built, every live `subscribe` trigger is re-subscribed onto it, the live backend
is swapped so new publishes route through it, and the previous backend's
subscriptions are torn down (aborting its redis tasks). The swap is gated — a
value that fails to build the backend keeps the previous one. Because pub/sub is
fire-and-forget, a publish in the brief window mid-swap may be observed by both
backends; prefer a quiet moment to repoint the adapter in a multi-instance
deployment.

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
