---
type: how-to
function_id: stream::get
title: Read one stream item by its full triple
---

# When to use

Call `stream::get` when you already know the exact `(stream_name,
group_id, item_id)` of the item you want and need its current persisted
value. The response is the raw `data` you previously stored, with no
envelope around it.

Reach for it when:

- A trigger payload handed you a specific `id` and you need the full
  body before reacting (the trigger only carries the new value, not the
  surrounding state).
- A UI received an `iii://iii-stream/<...>` reference and needs to inline
  the item.
- You're verifying the result of a `stream::set` or `stream::update`
  from a different process or instance.

Use [`stream::list`](iii://iii-stream/stream/listing) instead when you don't yet
have the `item_id` and want every item in the group. Use
[`stream::list_groups`](iii://iii-stream/stream/listing) to discover available
`group_id`s in a stream.

# Inputs

```json
{
  "stream_name": "presence",                   // required; top-level namespace
  "group_id":    "room-1",                     // required; second-level partition
  "item_id":     "user-123"                    // required; identifier within the group
}
```

All three fields are required. The worker treats them as opaque strings;
no validation beyond non-emptiness happens at the handler boundary.

# Outputs

```json
{ "online": true, "name": "Alice" }
```

- The response is the raw `data` value previously written via
  `stream::set` or `stream::update` — whatever JSON shape was stored
  there, returned unchanged.
- A missing item returns the JSON value `null` (not an error). Branch on
  `value === null` to distinguish "absent" from "stored as `null`"
  cannot be done with this function alone — the worker collapses both
  cases. If you need to disambiguate, structure your stored value to
  carry an explicit presence flag.
- On a custom override (registering a function at
  `stream::get(<stream_name>)`), the override's return value replaces
  the response verbatim.

# Worked example

Inline the presence record for `user-123` in `room-1`:

```json
{
  "stream_name": "presence",
  "group_id":    "room-1",
  "item_id":     "user-123"
}
```

The response is the raw stored value (object, array, scalar, or `null`) — see **Outputs** for the full coercion rules.

# Related

- `stream::list` — enumerate every item in a group when you don't have an `item_id` yet.
- `stream::list_groups` — discover which `group_id`s exist within a stream.
- `stream::set` — write the value back if you intend to mutate it.
- `stream::update` — atomically modify the value with op-based semantics rather than read-modify-write.
