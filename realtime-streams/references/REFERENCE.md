# Realtime Streams -- API Reference

Source: https://iii.dev/docs/how-to/stream-realtime-data

## Configuration

Add the StreamModule to `iii-config.yaml`:

```yaml
modules:
  - class: modules::stream::StreamModule
    config:
      port: ${STREAM_PORT:3112}
      host: localhost
      adapter:
        class: modules::stream::adapters::KvStore
        config:
          store_method: file_based
          file_path: ./data/stream_store
```

### Redis Adapter

For distributed deployments, use the Redis adapter:

```yaml
modules:
  - class: modules::stream::StreamModule
    config:
      port: ${STREAM_PORT:3112}
      host: localhost
      adapter:
        class: modules::stream::adapters::RedisAdapter
        config:
          redis_url: redis://localhost:6379
```

### Adapter Options

| Adapter | Config | Use Case |
|---------|--------|----------|
| `KvStore` (file_based) | `store_method: file_based`, `file_path: ./data/stream_store` | Local development, single-node |
| `KvStore` (in_memory) | `store_method: in_memory` | Testing, ephemeral data |
| `RedisAdapter` | `redis_url: redis://host:6379` | Production, multi-node |

## Stream Functions

### stream::set -- Write Data

Push data to a stream group and broadcast to connected WebSocket clients.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `stream_name` | string | Yes | Identifies the stream |
| `group_id` | string | Yes | Organizes data within a stream |
| `item_id` | string | Yes | Unique identifier for the item |
| `data` | object | Yes | Payload to store and broadcast |

### stream::get -- Read Single Item

Retrieve a single item from a stream group.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `stream_name` | string | Yes | Stream identifier |
| `group_id` | string | Yes | Group identifier |
| `item_id` | string | Yes | Item to retrieve |

### stream::list -- List Items

Retrieve all stored items in a stream group.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `stream_name` | string | Yes | Stream identifier |
| `group_id` | string | Yes | Group identifier |

**Returns:** Array of stored items.

### stream::delete -- Remove Item

Remove an item from a stream group.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `stream_name` | string | Yes | Stream identifier |
| `group_id` | string | Yes | Group identifier |
| `item_id` | string | Yes | Item to remove |

## TypeScript/Node.js

```typescript
import { init } from 'iii-sdk'

const iii = init('ws://localhost:49134')

// Write to stream
await iii.trigger({
  function_id: 'stream::set',
  payload: {
    stream_name: 'chat',
    group_id: 'room-123',
    item_id: 'msg-001',
    data: { text: 'Hello world', author: 'alice' },
  },
})

// List items
const messages = await iii.trigger({
  function_id: 'stream::list',
  payload: { stream_name: 'chat', group_id: 'room-123' },
})

// Get single item
const msg = await iii.trigger({
  function_id: 'stream::get',
  payload: { stream_name: 'chat', group_id: 'room-123', item_id: 'msg-001' },
})

// Delete item
await iii.trigger({
  function_id: 'stream::delete',
  payload: { stream_name: 'chat', group_id: 'room-123', item_id: 'msg-001' },
})
```

## Python

```python
from iii import init, InitOptions

iii = init('ws://localhost:49134', InitOptions(worker_name='stream-worker'))

# Write to stream
await iii.trigger({
    'function_id': 'stream::set',
    'payload': {
        'stream_name': 'chat',
        'group_id': 'room-123',
        'item_id': 'msg-001',
        'data': {'text': 'Hello world', 'author': 'alice'},
    },
})

# List items
messages = await iii.trigger({
    'function_id': 'stream::list',
    'payload': {'stream_name': 'chat', 'group_id': 'room-123'},
})

# Get single item
msg = await iii.trigger({
    'function_id': 'stream::get',
    'payload': {'stream_name': 'chat', 'group_id': 'room-123', 'item_id': 'msg-001'},
})

# Delete item
await iii.trigger({
    'function_id': 'stream::delete',
    'payload': {'stream_name': 'chat', 'group_id': 'room-123', 'item_id': 'msg-001'},
})
```

## Rust

```rust
use iii_sdk::{init, InitOptions, TriggerRequest};
use serde_json::json;

let iii = init("ws://127.0.0.1:49134", InitOptions::default())?;

// Write to stream
iii.trigger(TriggerRequest {
    function_id: "stream::set".into(),
    payload: json!({
        "stream_name": "chat",
        "group_id": "room-123",
        "item_id": "msg-001",
        "data": { "text": "Hello world", "author": "alice" }
    }),
    ..Default::default()
}).await?;

// List items
let messages = iii.trigger(TriggerRequest {
    function_id: "stream::list".into(),
    payload: json!({ "stream_name": "chat", "group_id": "room-123" }),
    ..Default::default()
}).await?;

// Get single item
let msg = iii.trigger(TriggerRequest {
    function_id: "stream::get".into(),
    payload: json!({ "stream_name": "chat", "group_id": "room-123", "item_id": "msg-001" }),
    ..Default::default()
}).await?;

// Delete item
iii.trigger(TriggerRequest {
    function_id: "stream::delete".into(),
    payload: json!({ "stream_name": "chat", "group_id": "room-123", "item_id": "msg-001" }),
    ..Default::default()
}).await?;
```

## WebSocket Client Connection

Clients connect via WebSocket to receive live updates:

```javascript
const ws = new WebSocket('ws://localhost:3112/stream/chat/room-123')

ws.onmessage = (event) => {
  const update = JSON.parse(event.data)
  console.log('New message:', update)
}

ws.onopen = () => console.log('Connected to stream')
ws.onclose = () => console.log('Disconnected from stream')
```

**Endpoint format:** `ws://{host}:{port}/stream/{stream_name}/{group_id}`
