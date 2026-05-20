---
type: how-to
function_id: state::set
title: Store or replace a value at a scope and key
---

# When to use

Call `state::set` when you have a complete value (object, array, or scalar) that should be written to a `scope`/`key` slot, replacing whatever was there before. It is the right call for first-time inserts and idempotent upserts where the new value is fully known.

Reach for it when:

- You are persisting a freshly-built record (user profile, order, session) under a known key.
- You want to publish a snapshot of a value so other workers can read it via `state::get`.
- You want to fan out a `state:created` or `state:updated` event to every registered `state` trigger.

Use [`state::update`](iii://iii-state/state/update) instead when you only need to change specific fields, increment a counter, or append to an array — `set` always replaces the entire value at `key`.

# Inputs

```json
{
  "scope": "users",       // required. Namespace that groups related keys.
  "key":   "user-123",    // required. Identifier within the scope.
  "value": {              // required. Any JSON-serializable value; replaces whatever is at scope/key.
    "name":  "Alice",
    "email": "alice@example.com"
  }
}
```

`scope`, `key`, and `value` are all required. The engine also accepts `data` as a backward-compatible alias for `value`; new callers should prefer `value`.

# Outputs

```json
{
  "old_value": null,                                  // The previous value at scope/key, or null when the key did not exist.
  "new_value": {                                      // The value that was stored. Echoes the input value.
    "name":  "Alice",
    "email": "alice@example.com"
  }
}
```

- `old_value` is `null` exactly when the key was newly created. Use it to distinguish create from update without a prior `state::get`.
- `new_value` always reflects what is now stored. For raw scalars, `value` and `new_value` are the same scalar JSON.

# Side effects

Every successful `state::set` invokes the engine's trigger fan-out for the `state` trigger type. Each matching trigger (filtered by optional `scope`, `key`, and `condition_function_id` on the trigger config) receives the following event payload:

```json
{
  "type":       "state",                       // Always "state".
  "event_type": "state:created",               // "state:created" when old_value was null, otherwise "state:updated".
  "scope":      "users",
  "key":        "user-123",
  "old_value":  null,                          // Previous value, or null for newly created keys.
  "new_value":  { "name": "Alice" }            // The stored value, matches the function's new_value.
}
```

- Trigger invocations are spawned asynchronously after the adapter write returns, so the caller does not wait for handlers to finish.
- A failing handler does not roll back the write; the value remains stored.

# Worked example

Persist a user profile so other functions can read it back:

```json
{
  "scope": "users",
  "key":   "user-123",
  "value": {
    "name":        "Alice",
    "email":       "alice@example.com",
    "preferences": { "theme": "dark" }
  }
}
```

Response when the key is brand new:

```json
{
  "old_value": null,
  "new_value": {
    "name":        "Alice",
    "email":       "alice@example.com",
    "preferences": { "theme": "dark" }
  }
}
```

# Related

- `state::get` — read the value back by `scope` and `key`.
- `state::update` — change individual fields without rewriting the whole document.
- `state::delete` — remove the key and fire `state:deleted`.
- `state::list` — enumerate every value in the scope after writing.
