# iii-stream

Durable streams for real-time data subscriptions. Streams organize data hierarchically: `stream_name` > `group_id` > `item_id`. Clients subscribe via WebSocket and receive real-time updates when items change.

When a worker triggers `stream::set`, the engine:
1. Persists the data via the configured adapter (Redis or KvStore)
2. Publishes a notification to all WebSocket clients subscribed to that stream and group
3. Evaluates registered `stream` triggers and fires matching handlers

## Sample Configuration

```yaml
- name: iii-stream
  config:
    port: ${STREAM_PORT:3112}
    host: 0.0.0.0
    adapter:
      name: redis
      config:
        redis_url: ${REDIS_URL:redis://localhost:6379}
```

## Configuration

| Field | Type | Description |
|---|---|---|
| `port` | number | The port to listen on. Defaults to `3112`. |
| `host` | string | The host to listen on. Defaults to `0.0.0.0`. |
| `auth_function` | string | Function ID to authenticate WebSocket connections. |
| `adapter` | Adapter | Adapter for stream storage and real-time delivery. |

## Adapters

### redis

Uses Redis as the backend. Stores stream data in Redis and uses Redis Pub/Sub for real-time delivery.

```yaml
name: redis
config:
  redis_url: ${REDIS_URL:redis://localhost:6379}
```

### kv

Built-in key-value store. Supports in-memory or file-based persistence. No external dependencies required.

```yaml
name: kv
config:
  store_method: file_based
  file_path: ./data/streams_store.db
```

| Field | Type | Description |
|---|---|---|
| `store_method` | string | `in_memory` (lost on restart) or `file_based` (persisted to disk). |
| `file_path` | string | Directory path for file-based storage. |

## Functions

### `stream::set`

Sets a value in the stream. Notifies all WebSocket subscribers and fires `stream` triggers.

Parameters: `stream_name` (string), `group_id` (string), `item_id` (string), `data` (any)

Returns: `old_value` (any), `new_value` (any)

### `stream::get`

Gets a value from the stream.

Parameters: `stream_name` (string), `group_id` (string), `item_id` (string)

Returns: `value` (any)

### `stream::delete`

Deletes a value from the stream.

Parameters: `stream_name` (string), `group_id` (string), `item_id` (string)

Returns: `old_value` (any)

### `stream::list`

Retrieves all items in a group.

Parameters: `stream_name` (string), `group_id` (string)

Returns: `group` (any[])

### `stream::list_groups`

Lists all groups in a stream.

Parameters: `stream_name` (string)

Returns: `groups` (string[])

### `stream::list_all`

Lists all streams with their group metadata.

Returns: `stream` (object[]), `count` (number)

### `stream::send`

Sends a custom event to all subscribers of a stream group (without persisting).

Parameters: `stream_name` (string), `group_id` (string), `type` (string), `data` (any), `id` (string, optional)

### `stream::update`

Atomically updates an item using a list of operations (`set`, `merge`, `increment`, `decrement`, `append`, `remove`).

Parameters: `stream_name` (string), `group_id` (string), `item_id` (string), `ops` (UpdateOp[])

Returns: `old_value` (any), `new_value` (any)

## Authentication

Define a function that receives request data (`headers`, `path`, `query_params`, `addr`) and returns `{ context: ... }`. Set it in config:

```yaml
- name: iii-stream
  config:
    auth_function: onAuth
```

**TypeScript:**
```typescript
iii.registerFunction('onAuth', (input) => ({
  context: { name: 'John Doe' },
}))
```

**Python:**
```python
def on_auth(input):
    return {'context': {'name': 'John Doe'}}

iii.register_function("onAuth", on_auth)
```

## Trigger Types

### `stream`

Fires when an item changes (via `stream::set`, `stream::update`, or `stream::delete`).

| Config Field | Type | Description |
|---|---|---|
| `stream_name` | string | Required. Only changes on this stream fire the handler. |
| `group_id` | string | If set, only changes within this group fire the handler. |
| `item_id` | string | If set, only changes to this specific item fire the handler. |
| `condition_function_id` | string | Function ID for conditional execution. |

Payload fields: `type` (`create`/`update`/`delete`), `timestamp`, `streamName`, `groupId`, `id`, `event` (object with `type` and `data`).

### `stream:join` and `stream:leave`

Fire when a client connects or disconnects via WebSocket.

Payload fields: `subscription_id`, `stream_name`, `group_id`, `id` (optional), `context` (from auth).

```typescript
const fn = iii.registerFunction('onJoin', (input) => {
  console.log(`Joined ${input.stream_name}/${input.group_id}`, input.context)
  return {}
})

iii.registerTrigger({
  type: 'stream:join',
  function_id: fn.id,
  config: {},
})
```

## Usage Example: Real-Time Presence

```typescript
import { registerWorker, TriggerAction } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

// Set presence
iii.trigger({
  function_id: 'stream::set',
  payload: {
    stream_name: 'presence',
    group_id: 'room-1',
    item_id: 'user-123',
    data: { name: 'Alice', online: true, lastSeen: new Date().toISOString() },
  },
  action: TriggerAction.Void(),
})

// Get a user
const user = await iii.trigger({
  function_id: 'stream::get',
  payload: { stream_name: 'presence', group_id: 'room-1', item_id: 'user-123' },
})

// List all members in a room
const roomMembers = await iii.trigger({
  function_id: 'stream::list',
  payload: { stream_name: 'presence', group_id: 'room-1' },
})
```

Clients connect via WebSocket to `ws://host:3112/stream/presence/room-1/` and receive real-time updates when items change.
