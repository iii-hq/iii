---
type: how-to
function_id: state::delete
title: Remove a key from a scope
---

# When to use

Call `state::delete` to remove the entry at a given `scope`/`key`. The function reads the prior value before deleting so callers can audit what was removed and so the `state:deleted` trigger event carries `old_value`.

Reach for it when:

- A user, session, or document is being permanently removed.
- You want to evict a cached value so the next `state::get` returns `null`.
- You want to fire a `state:deleted` event for downstream cleanup work (cache busts, audit logs, projections).

Use [`state::set`](iii://iii-state/state/set) instead when you want to overwrite the value rather than remove the key. Use [`state::update`](iii://iii-state/state/update) with a `remove` op when you only want to drop a single field from an object value while keeping the key.

# Inputs

```json
{
  "scope": "users",     // required. Namespace to delete from.
  "key":   "user-123"   // required. Key to remove.
}
```

Both fields are required. Deleting a missing key is not an error.

# Outputs

The function returns the value that was deleted, or `null` when the key did not exist.

```json
// When the key existed:
{
  "name":  "Alice",
  "email": "alice@example.com"
}
```

```json
// When the key did not exist:
null
```

- The return type mirrors whatever was stored at the key.
- A missing-key delete still completes successfully and returns `null`. The engine also fires a `state:deleted` event in that case — see Side effects.

# Side effects

A `state:deleted` event is fanned out to every matching `state` trigger after the adapter delete returns:

```json
{
  "type":       "state",            // Always "state".
  "event_type": "state:deleted",
  "scope":      "users",
  "key":        "user-123",
  "old_value":  { "name": "Alice" }, // The value that was removed.
  "new_value":  null                  // Always null for deletes.
}
```

- Trigger invocations are spawned asynchronously after the adapter delete returns.
- The event fires whenever the delete call succeeds, even when the key was already missing. In that case `old_value` is `null`. Handlers that should only react to real removals must short-circuit on `old_value == null`.

# Worked example

Remove a user profile and let any registered listeners react:

```json
{
  "scope": "users",
  "key":   "user-123"
}
```

# Related

- `state::set` — overwrite the value instead of deleting the key.
- `state::get` — confirm whether the key exists before deciding to delete.
- `state::update` — remove a single field while keeping the key.
- [React to state changes](iii://iii-state/state/reactive-triggers) — register a `state` trigger so another function runs when this delete fires an event.
