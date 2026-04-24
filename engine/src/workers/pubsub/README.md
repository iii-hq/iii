# iii-pubsub

Topic-based publish/subscribe messaging for broadcasting events to multiple subscribers in real time.

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
