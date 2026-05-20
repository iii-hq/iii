---
type: how-to
function_id: state::update
title: Apply atomic partial updates to a value
---

# When to use

Call `state::update` to mutate a value in place using a list of typed operations. The ops are applied atomically against the current value at `scope`/`key`, so concurrent callers cannot lose writes to each other the way a `state::get` + `state::set` cycle can.

Reach for it when:

- You need to increment a counter or running total.
- You want to shallow-merge a few fields into an existing object without rewriting the whole document.
- You want to append an element to an array (e.g. an event log) without reading it first.
- You need to remove a single field without losing the rest of the object.

Use [`state::set`](iii://iii-state/state/set) instead when the new value is fully known and should replace whatever is at `key`. Use [`state::delete`](iii://iii-state/state/delete) instead when you want to remove the key entirely rather than mutating its value.

# Inputs

```json
{
  "scope": "orders",          // required. Namespace that groups related keys.
  "key":   "order-42",        // required. Identifier within the scope.
  "ops": [                    // required. Ordered list of operations applied atomically.
    {
      "type":  "set",         // Op kind: set | merge | increment | decrement | append | remove.
      "path":  "status",      // Field to target. "" or omitted means the root value.
      "value": "paid"         // Required for set, merge, append; numeric `by` for increment/decrement.
    }
  ]
}
```

`scope`, `key`, and `ops` are required. Each op is a tagged object selected by `type`:

| Op | Shape | Effect |
|----|-------|--------|
| `set` | `{ "type": "set", "path": "status", "value": "paid" }` | Replace a first-level field, or the root when `path` is `""`. |
| `merge` | `{ "type": "merge", "path": "", "value": { "status": "paid" } }` | Shallow-merge an object at the root, a first-level field, or a nested path (array form). |
| `increment` | `{ "type": "increment", "path": "count", "by": 1 }` | Add `by` to a numeric first-level field. |
| `decrement` | `{ "type": "decrement", "path": "count", "by": 1 }` | Subtract `by` from a numeric first-level field. |
| `append` | `{ "type": "append", "path": "events", "value": { "kind": "chunk" } }` | Push one element onto an array, or concatenate to a string, at the root / first-level / nested path. |
| `remove` | `{ "type": "remove", "path": "status" }` | Delete a first-level field from the current object. |

Path rules:

- For `set`, `increment`, `decrement`, and `remove`, `path` is a single literal first-level field name. `"user.name"` writes the field literally named `user.name`; it does not traverse into a nested object.
- For `merge` and `append`, `path` may be a string (root or first-level field) or an array of literal segments for nested traversal: `["sessions", "abc", "events"]`. Missing or non-object intermediates are auto-created.
- `__proto__`, `constructor`, and `prototype` are rejected as path segments and merge-value top-level keys to prevent prototype pollution.

Limits: path depth ≤ 32 segments, segment ≤ 256 bytes, merge value depth ≤ 16, ≤ 1024 top-level keys in a merge value.

# Outputs

```json
{
  "old_value": {                          // Value before any op ran. Null if the key did not exist.
    "status": "pending",
    "count":  0
  },
  "new_value": {                          // Value after every successful op was applied.
    "status": "paid",
    "count":  1
  },
  "errors": [                             // Optional. Omitted when no op failed.
    {
      "op_index": 1,                      // Position of the failed op in the input `ops` array.
      "code":     "increment.not_number", // Stable error code; see Errors below.
      "message":  "Expected number at path 'name', got string.",
      "doc_url":  "https://iii.dev/docs/workers/iii-state#error-codes"
    }
  ]
}
```

- `old_value` is `null` exactly when the key did not exist before the call. The engine fires `state:created` in that case and `state:updated` otherwise.
- Updates are **best-effort**: each op is applied independently, and successful ops are reflected in `new_value` even if later ops fail. Failed ops are recorded in `errors` and skipped.
- The `errors` field is omitted entirely from the JSON when no op failed.

## Errors

Each entry in `errors` carries a stable `code`. The most common ones:

| Code | Trigger |
|------|---------|
| `set.target_not_object` / `append.target_not_object` / `increment.target_not_object` / `decrement.target_not_object` / `remove.target_not_object` | The op used a field `path` while the current value is not an object. Either initialize the root to an object with a prior `set` or target the root with `path: ""`. |
| `increment.not_number` / `decrement.not_number` | The targeted field is not a number. Initialize it with `set` to `0` first. |
| `append.type_mismatch` | The append value's type is incompatible with the existing field (e.g. appending a non-string to a string). |
| `<op>.path.proto_polluted` | A path segment is `__proto__`, `constructor`, or `prototype`. |
| `<op>.path.segment_too_long` / `merge.path.too_deep` / `append.path.too_deep` / `merge.path.empty_segment` / `append.path.empty_segment` | Path validation limits. |
| `merge.value.not_an_object` / `merge.value.too_deep` / `merge.value.too_many_keys` / `merge.value.proto_polluted` | Merge value validation. |

The full table lives at `https://iii.dev/docs/workers/iii-state#error-codes`.

# Side effects

After a successful update, the engine fans out either a `state:created` or `state:updated` event:

```json
{
  "type":       "state",                       // Always "state".
  "event_type": "state:updated",               // "state:created" when old_value was null.
  "scope":      "orders",
  "key":        "order-42",
  "old_value":  { "status": "pending", "count": 0 },
  "new_value":  { "status": "paid",    "count": 1 }
}
```

- The event is emitted even when some ops failed, as long as the adapter write succeeded. Subscribers must read `new_value` rather than reconstructing it from `ops`.
- Trigger handlers run asynchronously and do not roll back the update on failure.

# Worked example

Increment a counter and stamp the status in one call:

```json
{
  "scope": "orders",
  "key":   "order-42",
  "ops": [
    { "type": "increment", "path": "count",  "by": 1 },
    { "type": "set",       "path": "status", "value": "paid" }
  ]
}
```

Append an event to a nested log, auto-creating missing intermediates:

```json
{
  "scope": "orders",
  "key":   "order-42",
  "ops": [
    {
      "type":  "append",
      "path":  ["sessions", "abc", "events"],
      "value": { "kind": "chunk", "at": "2026-05-20T17:00:00Z" }
    }
  ]
}
```

# Related

- `state::set` — replace the whole value when partial updates do not fit.
- `state::get` — inspect the current value before designing an op list.
- `state::delete` — remove the key when it should no longer exist.
