# Trigger Conditions -- API Reference

Source: https://iii.dev/docs/how-to/use-trigger-conditions

## Execution Flow

1. Trigger fires (state change, HTTP request, queue message, etc.)
2. Engine calls the condition function with the event data
3. Condition returns `true` -- handler function executes
4. Condition returns `false` -- handler is skipped

## Condition Function Requirements

- Must be a registered function with a unique ID
- Must return a boolean (`true` or `false`)
- Receives the same event data the handler would receive

## Trigger Configuration

Add `condition_function_id` to any trigger's config:

```
config: {
  // ... trigger-type-specific config ...
  condition_function_id: 'conditions::my-condition'
}
```

Works with all trigger types: `state`, `http`, `queue`, `cron`, `stream`, `subscribe`.

## TypeScript/Node.js

```typescript
import { init } from 'iii-sdk'

const iii = init('ws://localhost:49134')

// Register the condition function
iii.registerFunction(
  { id: 'conditions::is-high-value' },
  async (input) => {
    return input.new_value?.amount >= 1000
  }
)

// Register the handler function
iii.registerFunction(
  { id: 'orders::notify-high-value' },
  async (input) => {
    // Only runs when condition returns true
    return { status_code: 200, body: { notified: true } }
  }
)

// Bind trigger with condition
iii.registerTrigger({
  type: 'state',
  function_id: 'orders::notify-high-value',
  config: {
    scope: 'orders',
    key: 'status',
    condition_function_id: 'conditions::is-high-value',
  },
})
```

## Python

```python
from iii import init, InitOptions

iii = init('ws://localhost:49134', InitOptions(worker_name='condition-worker'))

# Register the condition function
async def is_high_value(input):
    new_value = input.get('new_value', {})
    return new_value.get('amount', 0) >= 1000

iii.register_function('conditions::is-high-value', is_high_value)

# Register the handler function
async def notify_high_value(input):
    return {'status_code': 200, 'body': {'notified': True}}

iii.register_function('orders::notify-high-value', notify_high_value)

# Bind trigger with condition
iii.register_trigger({
    'type': 'state',
    'function_id': 'orders::notify-high-value',
    'config': {
        'scope': 'orders',
        'key': 'status',
        'condition_function_id': 'conditions::is-high-value',
    },
})
```

## Rust

```rust
use iii_sdk::{init, InitOptions, RegisterFunctionMessage};
use serde_json::json;

let iii = init("ws://127.0.0.1:49134", InitOptions::default())?;

// Register the condition function
iii.register_function(
    RegisterFunctionMessage { id: "conditions::is-high-value".into(), ..Default::default() },
    |input| async move {
        let amount = input["new_value"]["amount"].as_f64().unwrap_or(0.0);
        Ok(json!(amount >= 1000.0))
    }
);

// Register the handler function
iii.register_function(
    RegisterFunctionMessage { id: "orders::notify-high-value".into(), ..Default::default() },
    |input| async move {
        Ok(json!({"status_code": 200, "body": {"notified": true}}))
    }
);

// Bind trigger with condition
iii.register_trigger(json!({
    "type": "state",
    "function_id": "orders::notify-high-value",
    "config": {
        "scope": "orders",
        "key": "status",
        "condition_function_id": "conditions::is-high-value"
    }
}));
```

## Examples by Trigger Type

### State trigger with condition

```typescript
iii.registerTrigger({
  type: 'state',
  function_id: 'handler::fn',
  config: {
    scope: 'orders',
    key: 'status',
    condition_function_id: 'conditions::check',
  },
})
```

### HTTP trigger with condition

```typescript
iii.registerTrigger({
  type: 'http',
  function_id: 'handler::fn',
  config: {
    api_path: '/api/orders',
    http_method: 'POST',
    condition_function_id: 'conditions::validate-request',
  },
})
```

### Queue trigger with condition

```typescript
iii.registerTrigger({
  type: 'queue',
  function_id: 'handler::fn',
  config: {
    topic: 'order-events',
    condition_function_id: 'conditions::filter-event',
  },
})
```
