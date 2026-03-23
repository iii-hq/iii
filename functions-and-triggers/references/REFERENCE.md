# Functions & Triggers — API Reference

Source: https://iii.dev/docs/how-to/use-functions-and-triggers

## Register a Function

### TypeScript/Node.js

```typescript
import { init } from 'iii-sdk'

const iii = init('ws://localhost:49134')

iii.registerFunction({ id: 'namespace::name', description: 'optional' }, async (input) => {
  return { result: 'value' }
})
```

### Python

```python
from iii import init, InitOptions

iii = init('ws://localhost:49134', InitOptions(worker_name='my-worker'))

async def handler(input):
    return {'result': 'value'}

iii.register_function('namespace::name', handler)
```

### Rust

```rust
use iii_sdk::{init, InitOptions, RegisterFunctionMessage};
use serde_json::json;

let iii = init("ws://127.0.0.1:49134", InitOptions::default())?;

iii.register_function(
    RegisterFunctionMessage { id: "namespace::name".into(), ..Default::default() },
    |input| async move {
        Ok(json!({"result": "value"}))
    }
);
```

## Register Triggers

### HTTP

```typescript
iii.registerTrigger({
  type: 'http',
  function_id: 'namespace::name',
  config: { api_path: '/path', http_method: 'POST' },
})
```

### Queue

```typescript
iii.registerTrigger({
  type: 'queue',
  function_id: 'namespace::name',
  config: { topic: 'my-topic' },
})
```

### Cron (7-field format)

```typescript
iii.registerTrigger({
  type: 'cron',
  function_id: 'namespace::name',
  config: { expression: '0 0 * * * * *' }, // every hour
})
```

### State change

```typescript
iii.registerTrigger({
  type: 'state',
  function_id: 'namespace::name',
  config: { scope: 'my-scope', key: 'optional-key' },
})
```

### PubSub subscribe

```typescript
iii.registerTrigger({
  type: 'subscribe',
  function_id: 'namespace::name',
  config: { topic: 'event-topic' },
})
```

## Invoke Functions

```typescript
// Synchronous — wait for result
const result = await iii.trigger({ function_id: 'namespace::name', payload: { key: 'value' } })

// Fire-and-forget — don't wait
iii.trigger({ function_id: 'namespace::name', payload: data, action: TriggerAction.Void() })

// Enqueue — durable async with retries
const receipt = await iii.trigger({
  function_id: 'namespace::name',
  payload: data,
  action: TriggerAction.Enqueue({ queue: 'queue-name' }),
})
```

## HTTP-Invoked Functions (External Endpoints)

Register external HTTP endpoints as iii functions without a handler:

```typescript
iii.registerFunction(
  { id: 'notifications::send' },
  {
    url: 'https://hooks.example.com/notify',
    method: 'POST',
    timeout_ms: 5000,
    headers: { 'X-Service': 'iii-worker' },
    auth: { type: 'bearer', token_key: 'ENV_VAR_NAME' },
  }
)
```

Auth token keys reference environment variables resolved at invocation time.
