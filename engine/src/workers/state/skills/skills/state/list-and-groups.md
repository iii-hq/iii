---
type: how-to
functions: [state::list, state::list_groups]
title: Discover scopes and enumerate values
---

# When to use

Reach for these two functions when you do not yet know the keys (or even the scopes) you need to read. They are the discovery layer that sits above `state::get`: pick the right one from the question you are trying to answer.

| Question | Use this |
|----------|----------|
| Which scopes currently hold data? | `state::list_groups` |
| What are all the values inside a known scope? | `state::list` |
| What is the value at one specific `scope`/`key`? | [`state::get`](iii://iii-state/state/get) |

Common situations:

- Surfacing every record under a namespace (every user, every order) for a report or a bulk operation.
- Bootstrapping a UI that needs to render whatever scopes the engine currently knows about.
- Asserting in tests that a write left state in the expected shape.

Neither function takes a key filter; for a single point lookup, use `state::get` instead.

# `state::list`

## Inputs

```json
{
  "scope": "users"   // required. Namespace to enumerate values from.
}
```

`scope` is required. There is no pagination, key filter, or sort option — the function returns the full snapshot of the scope.

## Outputs

```json
[
  { "name": "Alice", "email": "alice@example.com" },
  { "name": "Bob",   "email": "bob@example.com" }
]
```

- The return is a flat JSON array of stored values, not a `{ key: value }` map. Keys are intentionally not exposed by this call — pair it with `state::get` when you need them.
- A missing or empty scope returns `[]` — the function does not distinguish "scope exists with no keys" from "scope was never created". Use `state::list_groups` first if you need that distinction.
- Iteration order is adapter-defined and should not be relied on; sort client-side if you need deterministic order.

# `state::list_groups`

## Inputs

```json
{}
```

The function takes no parameters. Send an empty object as the payload.

## Outputs

```json
{
  "groups": [        // Sorted, deduplicated list of scope names the adapter is tracking.
    "orders",
    "sessions",
    "users"
  ]
}
```

- `groups` is always present, sorted lexicographically (ascending), and deduplicated by the engine so each name appears at most once.
- Whether a scope that has been fully drained by `state::delete` still appears is adapter-defined. The built-in `kv` adapter keeps the scope name visible after all its keys are removed; the Redis adapter drops it automatically. Treat presence in `groups` as "the adapter has seen this scope," not "this scope has values" — pair with `state::list` (or `state::get`) if you need to confirm contents.

# Worked example

Discover the scopes, then read everything in one of them:

```json
// state::list_groups
{}
```

```json
// state::list against the picked scope
{ "scope": "users" }
```

# Related

- `state::get` — read one value by `scope`/`key` once you have it.
- `state::set` — populate a new scope so it shows up in `list_groups`.
- `state::update` — mutate one value in place after locating it via `list`.
