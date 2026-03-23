# State Reactions -- API Reference

Source: https://iii.dev/docs/how-to/react-to-state-changes

## Module Configuration

The state module must be enabled in `iii-config.yaml`:

```yaml
modules:
  - class: modules::state::StateModule
    config:
      adapter:
        class: modules::state::adapters::KvStore
        config:
          store_method: file_based  # or in_memory
          file_path: ./data/state_store.db
```

## State Change Event Shape

When a state trigger fires, the handler receives:

```json
{
  "key": "order-123",
  "new_value": { "status": "shipped", "trackingId": "TRK-456" },
  "old_value": { "status": "processing" },
  "scope": "orders",
  "event_type": "set"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `key` | string | The state key that changed |
| `new_value` | any | The value after the change |
| `old_value` | any | The value before the change (null if new) |
| `scope` | string | The state scope |
| `event_type` | string | Type of state operation (e.g., "set") |

## Register a Reaction Handler

### TypeScript/Node.js

```typescript
import { init, Logger, TriggerAction } from 'iii-sdk'

const iii = init('ws://localhost:49134')
const logger = new Logger()

iii.registerFunction({ id: 'orders::on-status-change' }, async (event) => {
  logger.info('Order status changed', {
    key: event.key,
    old: event.old_value,
    new: event.new_value,
  })

  if (event.new_value.status === 'shipped') {
    iii.trigger({
      function_id: 'notifications::order-shipped',
      payload: { orderId: event.key, trackingId: event.new_value.trackingId },
      action: TriggerAction.Void(),
    })
  }

  return { handled: true }
})
```

### Python

```python
import os
from iii import init, Logger, TriggerAction

iii = init(os.environ.get("III_URL", "ws://localhost:49134"))
logger = Logger()

def on_status_change(event):
    logger.info("Order status changed", {
        "key": event["key"],
        "old": event["old_value"],
        "new": event["new_value"]
    })

    if event["new_value"]["status"] == "shipped":
        iii.trigger({
            "function_id": "notifications::order-shipped",
            "payload": {"orderId": event["key"], "trackingId": event["new_value"]["trackingId"]},
            "action": TriggerAction.Void()
        })

    return {"handled": True}

iii.register_function({"id": "orders::on-status-change"}, on_status_change)
```

### Rust

```rust
use iii_sdk::{init, InitOptions, RegisterFunctionMessage, Logger, TriggerAction};
use serde_json::json;

let iii = init("ws://127.0.0.1:49134", InitOptions::default())?;
let logger = Logger::new();

iii.register_function(
    RegisterFunctionMessage {
        id: "orders::on-status-change".into(),
        ..Default::default()
    },
    |event| async move {
        logger.info("Order status changed", Some(&event));

        if event["new_value"]["status"] == "shipped" {
            iii.trigger(json!({
                "function_id": "notifications::order-shipped",
                "payload": {
                    "orderId": event["key"],
                    "trackingId": event["new_value"]["trackingId"]
                },
                "action": TriggerAction::Void {}
            }))?;
        }

        Ok(json!({ "handled": true }))
    },
);
```

## Register a State Trigger

### Watch all keys in a scope

```typescript
iii.registerTrigger({
  type: 'state',
  function_id: 'orders::on-status-change',
  config: { scope: 'orders' },
})
```

### Watch a specific key

```typescript
iii.registerTrigger({
  type: 'state',
  function_id: 'orders::on-status-change',
  config: { scope: 'orders', key: 'status' },
})
```

### Python

```python
iii.register_trigger({
    "type": "state",
    "function_id": "orders::on-status-change",
    "config": {"scope": "orders"}
})
```

### Rust

```rust
iii.register_trigger(RegisterTriggerInput {
    trigger_type: "state".into(),
    function_id: "orders::on-status-change".into(),
    config: json!({ "scope": "orders" }),
})?;
```

## Triggering a State Change

State reactions fire automatically when state is written:

```typescript
// This write triggers any registered state watchers on scope: 'orders'
await iii.trigger({
  function_id: 'state::set',
  payload: {
    scope: 'orders',
    key: 'order-123',
    value: { status: 'shipped', trackingId: 'TRK-456' },
  },
})
```

## Conditional Reactions

Use `condition_function_id` to filter which state changes invoke the handler:

```typescript
// Only react when order status becomes 'shipped'
iii.registerFunction({ id: 'conditions::is-shipped' }, async (event) => {
  return { match: event.new_value?.status === 'shipped' }
})

iii.registerTrigger({
  type: 'state',
  function_id: 'orders::on-shipped',
  config: {
    scope: 'orders',
    condition_function_id: 'conditions::is-shipped',
  },
})
```

## Complete Example: Order Lifecycle

```typescript
import { init, Logger, TriggerAction } from 'iii-sdk'

const iii = init('ws://localhost:49134')
const logger = new Logger()

// React to any order state change
iii.registerFunction({ id: 'orders::on-change' }, async (event) => {
  const { key, new_value, old_value } = event
  logger.info('Order changed', { orderId: key, from: old_value?.status, to: new_value.status })

  switch (new_value.status) {
    case 'confirmed':
      await iii.trigger({
        function_id: 'orders::process',
        payload: { orderId: key },
        action: TriggerAction.Enqueue({ queue: 'orders' }),
      })
      break
    case 'shipped':
      iii.trigger({
        function_id: 'notifications::send-shipping-email',
        payload: { orderId: key, trackingId: new_value.trackingId },
        action: TriggerAction.Void(),
      })
      break
    case 'delivered':
      await iii.trigger({
        function_id: 'state::update',
        payload: {
          scope: 'metrics',
          key: 'completed-orders',
          ops: [{ type: 'increment', path: 'count', by: 1 }],
        },
      })
      break
  }

  return { handled: true }
})

iii.registerTrigger({
  type: 'state',
  function_id: 'orders::on-change',
  config: { scope: 'orders' },
})

// HTTP endpoint to update order status (triggers the reaction)
iii.registerFunction({ id: 'orders::update-status' }, async (req) => {
  await iii.trigger({
    function_id: 'state::set',
    payload: {
      scope: 'orders',
      key: req.path_params.id,
      value: { ...req.body },
    },
  })
  return { status_code: 200, body: { updated: true } }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'orders::update-status',
  config: { api_path: '/orders/:id/status', http_method: 'PUT' },
})
```
