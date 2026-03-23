# State Management -- API Reference

Source: https://iii.dev/docs/how-to/manage-state

## Module Configuration

Enable the state module in `iii-config.yaml`:

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

## Payload Shapes

### state::set

```json
{ "scope": "users", "key": "user-123", "value": { "name": "Alice", "email": "alice@example.com" } }
```

### state::get

```json
{ "scope": "users", "key": "user-123" }
```

Returns: `{ "value": { "name": "Alice", "email": "alice@example.com" } }`

### state::update

```json
{
  "scope": "metrics",
  "key": "page-views",
  "ops": [
    { "type": "increment", "path": "total", "by": 1 },
    { "type": "set", "path": "last_seen", "value": "2025-01-15T10:00:00Z" }
  ]
}
```

### state::delete

```json
{ "scope": "users", "key": "user-123" }
```

### state::list

```json
{ "scope": "users" }
```

Returns: list of keys in the scope.

## Update Operations

| Op Type | Fields | Description |
|---------|--------|-------------|
| `set` | `path`, `value` | Assign value at the specified path |
| `increment` | `path`, `by` | Add numeric amount to value at path |

## TypeScript/Node.js Examples

```typescript
import { init, Logger } from 'iii-sdk'

const iii = init('ws://localhost:49134')
const logger = new Logger()

// Write state
await iii.trigger({
  function_id: 'state::set',
  payload: {
    scope: 'users',
    key: 'user-123',
    value: { name: 'Alice', email: 'alice@example.com', loginCount: 0 },
  },
})

// Read state
const result = await iii.trigger({
  function_id: 'state::get',
  payload: { scope: 'users', key: 'user-123' },
})
logger.info('User data', { user: result.value })

// Atomic update -- increment login count and set last login
await iii.trigger({
  function_id: 'state::update',
  payload: {
    scope: 'users',
    key: 'user-123',
    ops: [
      { type: 'increment', path: 'loginCount', by: 1 },
      { type: 'set', path: 'lastLogin', value: new Date().toISOString() },
    ],
  },
})

// Delete state
await iii.trigger({
  function_id: 'state::delete',
  payload: { scope: 'users', key: 'user-123' },
})

// List keys in scope
const keys = await iii.trigger({
  function_id: 'state::list',
  payload: { scope: 'users' },
})
```

## Python Examples

```python
import os
from iii import init, Logger

iii = init(os.environ.get("III_URL", "ws://localhost:49134"))
logger = Logger()

# Write state
iii.trigger({
    "function_id": "state::set",
    "payload": {
        "scope": "users",
        "key": "user-123",
        "value": {"name": "Alice", "email": "alice@example.com", "loginCount": 0}
    }
})

# Read state
result = iii.trigger({
    "function_id": "state::get",
    "payload": {"scope": "users", "key": "user-123"}
})
logger.info("User data", {"user": result["value"]})

# Atomic update
iii.trigger({
    "function_id": "state::update",
    "payload": {
        "scope": "users",
        "key": "user-123",
        "ops": [
            {"type": "increment", "path": "loginCount", "by": 1},
            {"type": "set", "path": "lastLogin", "value": "2025-01-15T10:00:00Z"}
        ]
    }
})

# Delete state
iii.trigger({
    "function_id": "state::delete",
    "payload": {"scope": "users", "key": "user-123"}
})
```

## Rust Examples

```rust
use iii_sdk::{init, InitOptions};
use serde_json::json;

let iii = init("ws://127.0.0.1:49134", InitOptions::default())?;

// Write state
iii.trigger(json!({
    "function_id": "state::set",
    "payload": {
        "scope": "users",
        "key": "user-123",
        "value": { "name": "Alice", "email": "alice@example.com", "loginCount": 0 }
    }
}))?;

// Read state
let result = iii.trigger(json!({
    "function_id": "state::get",
    "payload": { "scope": "users", "key": "user-123" }
}))?;

// Atomic update
iii.trigger(json!({
    "function_id": "state::update",
    "payload": {
        "scope": "users",
        "key": "user-123",
        "ops": [
            { "type": "increment", "path": "loginCount", "by": 1 },
            { "type": "set", "path": "lastLogin", "value": "2025-01-15T10:00:00Z" }
        ]
    }
}))?;

// Delete state
iii.trigger(json!({
    "function_id": "state::delete",
    "payload": { "scope": "users", "key": "user-123" }
}))?;
```

## Complete Example: User Profile Management

```typescript
import { init, Logger } from 'iii-sdk'

const iii = init('ws://localhost:49134')
const logger = new Logger()

// Create user profile via HTTP
iii.registerFunction({ id: 'users::create' }, async (req) => {
  const user = { id: crypto.randomUUID(), ...req.body, createdAt: new Date().toISOString() }

  await iii.trigger({
    function_id: 'state::set',
    payload: { scope: 'users', key: user.id, value: user },
  })

  return { status_code: 201, body: user }
})

// Get user profile
iii.registerFunction({ id: 'users::get' }, async (req) => {
  const result = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'users', key: req.path_params.id },
  })

  if (!result.value) {
    return { status_code: 404, body: { error: 'User not found' } }
  }
  return { status_code: 200, body: result.value }
})

// Track login
iii.registerFunction({ id: 'users::record-login' }, async (payload) => {
  await iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'users',
      key: payload.userId,
      ops: [
        { type: 'increment', path: 'loginCount', by: 1 },
        { type: 'set', path: 'lastLogin', value: new Date().toISOString() },
      ],
    },
  })
  return { updated: true }
})

iii.registerTrigger({ type: 'http', function_id: 'users::create', config: { api_path: '/users', http_method: 'POST' } })
iii.registerTrigger({ type: 'http', function_id: 'users::get', config: { api_path: '/users/:id', http_method: 'GET' } })
```

## Key Constraints

- State is shared across all functions -- use meaningful scope names to avoid collisions
- `file_based` adapter is for single-instance deployments only
- `in_memory` adapter loses data on restart (development only)
- Production multi-instance setups require persistent adapters (e.g., Redis)
