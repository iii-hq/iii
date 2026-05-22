---
type: how-to
function_id: state::get
title: Read one value by scope and key
---

# When to use

Call `state::get` when you already know the `scope` and `key` and need the stored value. It is the cheapest read path — a single point lookup with no scan.

Reach for it when:

- You are loading a record by id inside a handler (user profile, session, configuration).
- You want to confirm whether a key exists before deciding to `state::set` or `state::update`.
- You want the value as it stands right now, after any recent writes and triggers.

Use [`state::list`](iii://iii-state/state/list-and-groups) instead when you need every value in a scope and do not know the keys. Use [`state::list_groups`](iii://iii-state/state/list-and-groups) instead when you do not yet know which scopes exist.

# Inputs

```json
{
  "scope": "users",     // required. Namespace to read from.
  "key":   "user-123"   // required. Key whose value should be returned.
}
```

Both fields are required.

# Outputs

The function returns the stored value directly — not an object wrapping it. Missing keys return JSON `null`.

```json
// When the key exists:
{
  "name":  "Alice",
  "email": "alice@example.com"
}
```

```json
// When the key does not exist:
null
```

- The return type mirrors whatever was stored: objects, arrays, scalars (strings, numbers, booleans), or `null` are all possible.
- A genuine stored value of `null` is indistinguishable from a missing key over the wire. If that matters, check the key first with `state::list` or track existence in a separate field.

# Worked example

Read a user profile after another function set it:

```json
{
  "scope": "users",
  "key":   "user-123"
}
```

# Related

- `state::set` — write the value before reading it.
- `state::update` — apply partial changes after inspecting the current value.
- `state::list` — read every value in the scope when keys are unknown.
- `state::list_groups` — discover which scopes contain data.
