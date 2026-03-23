# Custom Triggers -- API Reference

Source: https://iii.dev/docs/how-to/create-custom-trigger-type

## TriggerConfig Type

Both `registerTrigger` and `unregisterTrigger` callbacks receive this object:

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Trigger instance identifier |
| `function_id` | string | The function bound to this trigger |
| `config` | object | Caller-supplied configuration (custom per trigger type) |

## registerTriggerType

Registers a custom trigger type with the engine.

**Parameters:**
- Configuration: `{ id: string, description: string }`
- Handler: object implementing `registerTrigger` and `unregisterTrigger` callbacks

## unregisterTriggerType

Removes a custom trigger type. All bound triggers cease firing.

**Parameters:**
- Configuration: `{ id: string, description: string }`

## Built-in Trigger Types

These are provided by the engine and do not need custom registration:

| Type | Event Source |
|------|-------------|
| `http` | HTTP requests to API paths |
| `cron` | Scheduled time expressions |
| `queue` | Messages on named queues |
| `state` | State key changes |
| `stream` | Stream data updates |
| `subscribe` | PubSub topic messages |

## TypeScript/Node.js

```typescript
import { init } from 'iii-sdk'

const iii = init('ws://localhost:49134')

// Define the trigger handler
const webhookHandler = {
  async registerTrigger(config: { id: string; function_id: string; config: { path: string } }) {
    // Set up a listener for the event source
    // e.g., start an HTTP server, watch a file, connect to IoT broker
    const server = createServer(config.config.path, async (body) => {
      // Dispatch the event to the bound function
      await iii.trigger({
        function_id: config.function_id,
        payload: body,
      })
    })
    servers.set(config.id, server)
  },

  async unregisterTrigger(config: { id: string; function_id: string; config: { path: string } }) {
    // Clean up resources
    const server = servers.get(config.id)
    if (server) {
      server.close()
      servers.delete(config.id)
    }
  },
}

// Register the custom trigger type
iii.registerTriggerType(
  { id: 'webhook', description: 'Custom webhook trigger' },
  webhookHandler
)

// Now functions can bind to it
iii.registerTrigger({
  type: 'webhook',
  function_id: 'orders::on-webhook',
  config: { path: '/webhooks/orders' },
})

// Remove the trigger type when no longer needed
iii.unregisterTriggerType({ id: 'webhook', description: 'Custom webhook trigger' })
```

## Python

```python
from iii import init, InitOptions

iii = init('ws://localhost:49134', InitOptions(worker_name='trigger-worker'))

class WebhookHandler:
    def __init__(self):
        self.servers = {}

    async def register_trigger(self, config):
        # config has id, function_id, config
        # Set up listener for the event source
        async def on_event(body):
            await iii.trigger({
                'function_id': config['function_id'],
                'payload': body,
            })

        server = create_server(config['config']['path'], on_event)
        self.servers[config['id']] = server

    async def unregister_trigger(self, config):
        # Clean up resources
        server = self.servers.pop(config['id'], None)
        if server:
            server.close()

handler = WebhookHandler()

# Register the custom trigger type
iii.register_trigger_type(
    {'id': 'webhook', 'description': 'Custom webhook trigger'},
    handler
)

# Bind a function to the custom trigger
iii.register_trigger({
    'type': 'webhook',
    'function_id': 'orders::on-webhook',
    'config': {'path': '/webhooks/orders'},
})

# Remove when done
iii.unregister_trigger_type({'id': 'webhook', 'description': 'Custom webhook trigger'})
```

## Rust

```rust
use iii_sdk::{init, InitOptions, TriggerConfig, TriggerHandler, IIIError};
use async_trait::async_trait;
use serde_json::json;

struct WebhookHandler {
    // Store active listeners
}

#[async_trait]
impl TriggerHandler for WebhookHandler {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        // Set up listener for the event source
        // config.id, config.function_id, config.config available
        // When event fires: iii.trigger(TriggerRequest { function_id: config.function_id, ... })
        Ok(())
    }

    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), IIIError> {
        // Tear down resources for this trigger instance
        Ok(())
    }
}

let iii = init("ws://127.0.0.1:49134", InitOptions::default())?;

let handler = WebhookHandler {};

// Register the custom trigger type
iii.register_trigger_type(
    json!({ "id": "webhook", "description": "Custom webhook trigger" }),
    handler,
);

// Bind a function
iii.register_trigger(json!({
    "type": "webhook",
    "function_id": "orders::on-webhook",
    "config": { "path": "/webhooks/orders" }
}));

// Remove when done
iii.unregister_trigger_type(json!({ "id": "webhook", "description": "Custom webhook trigger" }));
```

## Use Cases

| Use Case | registerTrigger Action | Config Example |
|----------|----------------------|----------------|
| Webhooks | Start HTTP listener on custom path | `{ path: '/hooks/stripe' }` |
| File watcher | Start filesystem watcher | `{ directory: '/uploads', pattern: '*.csv' }` |
| IoT / MQTT | Subscribe to MQTT topic | `{ broker: 'mqtt://...', topic: 'sensors/#' }` |
| Database CDC | Connect to change stream | `{ connection: '...', table: 'orders' }` |
| Polling | Start interval timer | `{ url: 'https://api.example.com', interval_ms: 30000 }` |
