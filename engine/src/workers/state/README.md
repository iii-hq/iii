# iii-state

Distributed key-value state storage with scope-based organization and reactive triggers that fire on any state change.

State is server-side key-value storage with trigger-based reactivity. Unlike streams, state does not push updates to WebSocket clients — it fires triggers that workers handle server-side.

## Sample Configuration

```yaml
- name: iii-state
  config:
    adapter:
      name: kv
      config:
        store_method: file_based
        file_path: ./data/state_store
        save_interval_ms: 5000
```

## Configuration

| Field | Type | Description |
|---|---|---|
| `adapter` | Adapter | Adapter for state persistence. Defaults to `kv`. |

## Adapters

### kv

Built-in key-value store with in-memory or file-based persistence.

```yaml
name: kv
config:
  store_method: file_based
  file_path: ./data/state_store
  save_interval_ms: 5000
```

| Field | Type | Description |
|---|---|---|
| `store_method` | string | `in_memory` (lost on restart) or `file_based` (persisted to disk). |
| `file_path` | string | Directory path for file-based storage. |
| `save_interval_ms` | number | Interval in milliseconds between automatic disk saves. Defaults to `5000`. |

### redis

Uses Redis as the state backend.

```yaml
name: redis
config:
  redis_url: ${REDIS_URL:redis://localhost:6379}
```

### bridge

Forwards state operations to a remote III Engine instance via the Bridge Client.

```yaml
name: bridge
```

## Functions

### `state::set`

Set a value in state. Fires `state:created` if the key did not exist, or `state:updated` if it did.

Parameters: `scope` (string), `key` (string), `value` (any)

Returns: `old_value` (any), `new_value` (any)

### `state::get`

Get a value from state.

Parameters: `scope` (string), `key` (string)

Returns: `value` (any), or `null` if the key does not exist.

### `state::delete`

Delete a value from state. Fires a `state:deleted` trigger.

Parameters: `scope` (string), `key` (string)

Returns: `value` (the deleted value, or `null`)

### `state::update`

Atomically update a value using one or more operations. Fires `state:created` or `state:updated`.

Parameters: `scope` (string), `key` (string), `ops` (UpdateOp[])

| Operation | Shape | Description |
|-----------|-------|-------------|
| `set` | `{ "type": "set", "path": "status", "value": "active" }` | Set a field or replace the root value. |
| `merge` | `{ "type": "merge", "path": "", "value": { "status": "active" } }` | Shallow-merge an object at the root. |
| `increment` | `{ "type": "increment", "path": "count", "by": 1 }` | Add `by` to a numeric field. |
| `decrement` | `{ "type": "decrement", "path": "count", "by": 1 }` | Subtract `by` from a numeric field. |
| `append` | `{ "type": "append", "path": "events", "value": { "kind": "chunk" } }` | Push one element to an array or concatenate a string. |
| `remove` | `{ "type": "remove", "path": "status" }` | Remove a field from the current object. |

Returns: `old_value` (any), `new_value` (any)

### `state::list`

List all values within a scope.

Parameters: `scope` (string)

Returns: a flat JSON array of all stored values within the scope.

### `state::list_groups`

List all scopes that contain state data.

Returns: `groups` (string[]) — sorted, deduplicated array of all scope names.

## Trigger Type: `state`

Fires when a state value is created, updated, or deleted.

| Config Field | Type | Description |
|---|---|---|
| `scope` | string | Only fire for changes within this scope. When omitted, fires for all scopes. |
| `key` | string | Only fire for changes to this specific key. When omitted, fires for all keys. |
| `condition_function_id` | string | Function ID for conditional execution. If it returns `false`, the handler is skipped. |

### State Event Payload

| Field | Type | Description |
|---|---|---|
| `type` | string | Always `"state"`. |
| `event_type` | string | `"state:created"`, `"state:updated"`, or `"state:deleted"`. |
| `scope` | string | The scope where the change occurred. |
| `key` | string | The key that changed. |
| `old_value` | any | The previous value, or `null` for newly created keys. |
| `new_value` | any | The new value. `null` for deleted keys. |

### Sample Code

```typescript
const fn = iii.registerFunction(
  { id: 'state::onUserUpdated' },
  async (event) => {
    console.log('State changed:', event.event_type, event.key)
    console.log('Previous:', event.old_value)
    console.log('Current:', event.new_value)
    return {}
  },
)

iii.registerTrigger({
  type: 'state',
  function_id: fn.id,
  config: { scope: 'users', key: 'profile' },
})
```
