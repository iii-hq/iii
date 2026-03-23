# HTTP Endpoints -- API Reference

Source: https://iii.dev/docs/how-to/expose-http-endpoint

## Module Configuration

Enable the REST API module in `iii-config.yaml`:

```yaml
modules:
  - class: modules::api::RestApiModule
    config:
      port: 3111
      host: localhost
      default_timeout: 30000
      concurrency_request_limit: 1024
      cors:
        allowed_origins:
          - localhost
        allowed_methods: [GET, POST, PUT, DELETE, OPTIONS]
```

## Request / Response Shape

### ApiRequest (handler input)

```typescript
{
  body: any,            // parsed request body
  path_params: object,  // e.g. { id: '123' } for /users/:id
  headers: object,      // request headers
  method: string,       // GET, POST, PUT, DELETE, etc.
}
```

### ApiResponse (handler return)

```typescript
{
  status_code: number,  // HTTP status code
  body: any,            // response body (serialized as JSON)
  headers?: object,     // optional response headers
}
```

## Register a Function

### TypeScript/Node.js

```typescript
import { init, Logger } from 'iii-sdk'

const iii = init('ws://localhost:49134')
const logger = new Logger()

iii.registerFunction({ id: 'users::create' }, async (req) => {
  logger.info('Creating user', { body: req.body })
  const user = { id: crypto.randomUUID(), ...req.body }
  return { status_code: 201, body: user }
})
```

### Python

```python
import os
from iii import init, Logger

iii = init(os.environ.get("III_URL", "ws://localhost:49134"))
logger = Logger()

def create_user(req):
    logger.info("Creating user", {"body": req["body"]})
    import uuid
    user = {"id": str(uuid.uuid4()), **req["body"]}
    return {"status_code": 201, "body": user}

iii.register_function({"id": "users::create"}, create_user)
```

### Rust

```rust
use iii_sdk::{init, InitOptions, RegisterFunctionMessage, Logger};
use serde_json::json;

let iii = init("ws://127.0.0.1:49134", InitOptions::default())?;
let logger = Logger::new();

iii.register_function(
    RegisterFunctionMessage { id: "users::create".into(), ..Default::default() },
    |req| async move {
        logger.info("Creating user", Some(&req));
        let user = json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "name": req["body"]["name"],
            "email": req["body"]["email"]
        });
        Ok(json!({ "status_code": 201, "body": user }))
    },
);
```

## Register an HTTP Trigger

### TypeScript/Node.js

```typescript
iii.registerTrigger({
  type: 'http',
  function_id: 'users::create',
  config: { api_path: '/users', http_method: 'POST' },
})
```

### Python

```python
iii.register_trigger({
    "type": "http",
    "function_id": "users::create",
    "config": {"api_path": "/users", "http_method": "POST"}
})
```

### Rust

```rust
iii.register_trigger(RegisterTriggerInput {
    trigger_type: "http".into(),
    function_id: "users::create".into(),
    config: json!({ "api_path": "/users", "http_method": "POST" }),
})?;
```

## Path Parameters

Use `:param` syntax in `api_path`:

```typescript
iii.registerTrigger({
  type: 'http',
  function_id: 'users::get',
  config: { api_path: '/users/:id', http_method: 'GET' },
})

iii.registerFunction({ id: 'users::get' }, async (req) => {
  const userId = req.path_params.id
  return { status_code: 200, body: { id: userId, name: 'Example' } }
})
```

## Multiple Routes Example

```typescript
import { init } from 'iii-sdk'

const iii = init('ws://localhost:49134')

// POST /users
iii.registerFunction({ id: 'users::create' }, async (req) => {
  return { status_code: 201, body: { id: '1', ...req.body } }
})
iii.registerTrigger({
  type: 'http',
  function_id: 'users::create',
  config: { api_path: '/users', http_method: 'POST' },
})

// GET /users/:id
iii.registerFunction({ id: 'users::get' }, async (req) => {
  return { status_code: 200, body: { id: req.path_params.id } }
})
iii.registerTrigger({
  type: 'http',
  function_id: 'users::get',
  config: { api_path: '/users/:id', http_method: 'GET' },
})

// DELETE /users/:id
iii.registerFunction({ id: 'users::delete' }, async (req) => {
  return { status_code: 204, body: null }
})
iii.registerTrigger({
  type: 'http',
  function_id: 'users::delete',
  config: { api_path: '/users/:id', http_method: 'DELETE' },
})
```

## Calling Other Functions from an HTTP Handler

```typescript
iii.registerFunction({ id: 'users::create' }, async (req) => {
  const user = { id: crypto.randomUUID(), ...req.body }

  // Persist via state
  await iii.trigger({
    function_id: 'state::set',
    payload: { scope: 'users', key: user.id, value: user },
  })

  // Fire-and-forget notification
  iii.trigger({
    function_id: 'notifications::welcome',
    payload: { userId: user.id },
    action: TriggerAction.Void(),
  })

  return { status_code: 201, body: user }
})
```
