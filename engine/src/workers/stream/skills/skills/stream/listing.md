---
type: how-to
functions: [stream::list, stream::list_groups, stream::list_all]
title: Enumerate items, groups, and streams
---

# When to use

The three listing functions are different scopes of the same intent:
"show me what's there." Pick by which level of the hierarchy you have
and which level you want.

| Question                                                  | Use this                |
|-----------------------------------------------------------|-------------------------|
| What items live inside a known `(stream, group)` pair?    | `stream::list`          |
| Which groups exist inside a known stream?                 | `stream::list_groups`   |
| Which streams exist on this engine, and what groups each? | `stream::list_all`      |

A typical discovery flow is `list_all` → pick a `stream` → `list_groups`
→ pick a `group_id` → `list` → pick an item — and only then call
[`stream::get`](iii://iii-stream/stream/get) on a specific id.

Use [`stream::get`](iii://iii-stream/stream/get) instead when you already know
the full `(stream_name, group_id, item_id)` triple and want one item
back, not a collection.

# `stream::list`

## Inputs

```json
{
  "stream_name": "presence",                   // required; top-level namespace
  "group_id":    "room-1"                      // required; second-level partition whose items should be listed
}
```

## Outputs

```json
[
  { "online": true,  "name": "Alice" },
  { "online": false, "name": "Bob"   }
]
```

- The response is a JSON array of stored values — each element is the
  raw `data` previously written via `stream::set` / `stream::update`,
  with no surrounding envelope.
- An empty group returns `[]`, not `null`.
- A missing group also returns `[]` — there's no separate "no such
  group" signal; combine with `stream::list_groups` if you need to
  distinguish "exists but empty" from "doesn't exist".
- Item ids are **not** included in the response. If your data needs to
  carry its id, store it as a field on the value (`{ id: "user-123",
  ... }`) before writing.

# `stream::list_groups`

## Inputs

```json
{
  "stream_name": "presence"                    // required; top-level namespace
}
```

## Outputs

```json
["room-1", "room-2", "room-3"]
```

- The response is a JSON array of `group_id` strings.
- Order is adapter-defined and **not guaranteed** to be stable across
  calls or across adapters (`kv` and `redis` differ). Sort client-side
  if you display these to a user.
- A missing or empty stream returns `[]`.

# `stream::list_all`

## Inputs

```json
{}
```

`list_all` takes no inputs — it always returns every stream the engine
knows about.

## Outputs

```json
{
  "stream": [                                  // metadata for every stream
    { "id": "presence", "groups": ["room-1", "room-2"] },
    { "id": "audit",    "groups": ["user-123"]         }
  ],
  "count":  2                                  // == stream.length; convenience field
}
```

- `stream` is a JSON array of `{ id, groups }` objects, one per
  stream that has at least one stored item.
- `count` is exactly `stream.length`; included so callers can branch on
  presence without re-measuring.
- Streams that exist purely as registered triggers (no items written
  yet) **do not** appear here — `list_all` reads from the adapter, not
  from the trigger registry.

# Worked example

The end-to-end discovery flow chains all three:

```json
{}
```

Call `stream::list_all` with that empty input to enumerate every stream and its groups, pick a `stream_name` from the response, pass it to `stream::list_groups` to enumerate that stream's `group_id`s, then pass the `(stream_name, group_id)` pair to `stream::list` for the items themselves. From there reach for [`stream::get`](iii://iii-stream/stream/get) with a specific `item_id`, or [`stream::set`](iii://iii-stream/stream/set) / [`stream::update`](iii://iii-stream/stream/update) to mutate.

# Related

- `stream::get` — read a specific item once you've narrowed down to a `(stream, group, item)` triple.
- `stream::set` — write an item; afterwards it shows up under all three listings.
- `stream::delete` — remove an item; afterwards it disappears from `stream::list`, and the group disappears from `list_groups` once the last item is gone.
