# iii-stream

> **Deprecated.** iii-stream is deprecated (iii-stream) and will be removed in an upcoming release.
> Behavior is unchanged for now. Migration guide:
> https://iii.dev/docs/upgrading/migrate-from-streams

Durable streams for real-time data subscriptions. Streams organize data hierarchically: `stream_name` > `group_id` > `item_id`. Clients subscribe via WebSocket and receive real-time updates when items change.

When a worker calls `stream::set`, the engine:
1. Persists the data via the configured adapter (Redis or KvStore)
2. Publishes a notification to all WebSocket clients subscribed to that stream and group
3. Calls `stream::on-changed`, whose matching observers run

`iii-stream` is engine-owned. Configure it under `engine.workers.iii-stream` in a managed
`worker-compose.yaml`.

## Skills

Install the `iii-stream` agent skill for Claude Code, Cursor, and 30+ other agents:

```bash
npx skills add iii-hq/iii --full-depth --skill iii-stream
```

## Sample Configuration

```yaml
engine:
  workers:
    iii-stream:
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

## Runtime configuration (hot reload)

`iii-stream` registers its configuration with the builtin `configuration` worker
under the id **`iii-stream`**, so the fields above can be read and changed at
runtime (e.g. `configuration::set { id: "iii-stream", value: { ... } }`) without
restarting the engine. The `engine.workers` block is the **seed** used on first boot
only; afterwards the configuration entry is the runtime source of truth and a
runtime edit survives engine restarts. Values are validated against the schema
at set time, and `${VAR:default}` placeholders are expanded on read.

Each field applies on its own tier:

- **`auth_function`** applies to new connections immediately — no rebind. (It is
  consulted only when a client connects, so existing connections are unaffected.)
- **`host`/`port`** cause a **listener rebind**: the new address is bound, the
  WebSocket server is respawned on it, and the old listener is torn down. Live
  connections on the old address are dropped (clients reconnect). The rebind is
  gated — a value that fails to bind keeps the previous server running. Note that
  changing **only the host on the same port** between overlapping interfaces (e.g.
  `0.0.0.0` ↔ `127.0.0.1`) won't rebind: the new address can't bind while the old
  listener still holds the port, so the change is logged and the previous server
  kept. Change the port too, or restart, to move between overlapping interfaces.
- **`adapter`** causes a **full backend hot-swap**: the new pub/sub backend is
  built, swapped in, and its event pump restarted. New connections use it; the
  swap is gated (a value that fails to build the backend keeps the previous one).
  Existing connections remain bound to the **previous** backend until they close,
  so they no longer receive new events — prefer a quiet moment to repoint the
  adapter in a multi-instance deployment.

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

Sets a value in the stream. Notifies all WebSocket subscribers and calls `stream::on-changed`.

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

## Engine-hosted sources

Private functions only the engine calls, each with its event as input and output. Observe them to
react to stream activity; filter on `input.stream_name`, `input.group_id`, and `input.id`.

### `stream::on-changed`

Called when an item changes (via `stream::set`, `stream::update`, or `stream::delete`), a group is
synced, or a custom event is sent to a group (`stream::send`).

Event fields: `event_type` (`create`/`update`/`delete`/`sync`/`event`), `timestamp`, `stream_name`,
`group_id`, `id`, `event` (the change as stream subscribers receive it).

```typescript
iii.observe(
  {
    function_id: 'stream::on-changed',
    filters: { 'input.stream_name': { $eq: 'presence' }, 'input.group_id': { $eq: 'room-1' } },
  },
  'presence::on-change',
)
```

### `stream::on-join` and `stream::on-leave`

Called when a client subscribes to a group (or an item) over WebSocket, and when it leaves.

Event fields: `subscription_id`, `stream_name`, `group_id`, `id` (optional), `context` (from auth).

```typescript
iii.registerFunction('presence::on-join', ({ input }) => {
  console.log(`Joined ${input.stream_name}/${input.group_id}`, input.context)
  return {}
})

iii.observe('stream::on-join', 'presence::on-join')
```

Authorize subscriptions in the worker's `auth_function`, which runs once per WebSocket handshake.

## Usage Example: Real-Time Presence

```typescript
import { CallAction, registerWorker } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

// Set presence
iii.call({
  function_id: 'stream::set',
  payload: {
    stream_name: 'presence',
    group_id: 'room-1',
    item_id: 'user-123',
    data: { name: 'Alice', online: true, lastSeen: new Date().toISOString() },
  },
  action: CallAction.Void(),
})

// Get a user
const user = await iii.call({
  function_id: 'stream::get',
  payload: { stream_name: 'presence', group_id: 'room-1', item_id: 'user-123' },
})

// List all members in a room
const roomMembers = await iii.call({
  function_id: 'stream::list',
  payload: { stream_name: 'presence', group_id: 'room-1' },
})
```

## Client Subscriptions

Browser and client subscriptions use the Browser SDK (`iii-browser-sdk`), which subscribes to `stream` changes over a single engine WebSocket and re-renders on each change event. Connections are gated by the [iii-worker-manager](https://workers.iii.dev/workers/iii-worker-manager) RBAC listener. See the [Linkly frontend tutorial](https://iii.dev/docs/tutorials/linkly/frontend) for the end-to-end pattern.

Connecting directly to the stream port (`ws://host:3112/stream/<stream_name>/<group_id>/`) is deprecated in favor of the Browser SDK.
