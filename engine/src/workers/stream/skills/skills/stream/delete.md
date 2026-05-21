---
type: how-to
function_id: stream::delete
title: Remove a stream item and broadcast a delete event
---

# When to use

Call `stream::delete` when you want to remove an item from the persistent
store and notify every WebSocket subscriber and every registered
`stream::*` trigger of the removal in the same call. The response carries
the deleted value so callers can log, audit, or undo the removal without
a preceding `stream::get`.

Reach for it when:

- A user logs out / leaves a room and the presence record should
  disappear for everyone watching.
- A job completes and its in-flight status item should be cleared.
- A trigger registered against this `(stream, group, item)` should fire
  on removal.

Use [`stream::set`](iii://iii-stream/stream/set) with a sentinel value (e.g.
`{ archived: true }`) instead when you need to keep the item visible to
listing endpoints but mark it as no longer active. `delete` is final —
once the broadcast goes out, there's no record left in the adapter.

# Inputs

```json
{
  "stream_name": "presence",                   // required; top-level namespace
  "group_id":    "room-1",                     // required; second-level partition
  "item_id":     "user-123"                    // required; identifier within the group
}
```

All three fields are required. Deleting a missing item returns
`old_value: null` and **does not** broadcast or fire triggers — the
operation is idempotent on absent items.

# Outputs

```json
{
  "old_value": { "online": true, "name": "Alice" }   // the value that was removed; null if the item didn't exist
}
```

- `old_value` is the full JSON value that lived at the location, or
  `null` when the item didn't exist before the call. Use it to log or
  emit a tombstone record if your downstream needs the body of the
  removed item.
- Unlike `stream::set` and `stream::update`, the response carries no
  `new_value` field — the new value is "absent", not a JSON value.

# Side effects

A successful delete that actually removed something produces three
effects:

1. **Synchronous, before the call returns** — the adapter removes the
   entry at `(stream_name, group_id, item_id)`. Subsequent
   `stream::get` calls return `null`; `stream::list` no longer
   includes the item.
2. **Asynchronous, fire-and-forget** — the worker spawns a task that
   invokes every registered [`stream`](iii://iii-stream/stream/reactive-triggers)
   trigger whose `(stream_name, group_id, item_id)` filter matches.
   Handlers run on that spawned task **after** the `delete` call
   returns. Slow or failing handlers do not delay or fail the
   originating `delete`. Per-handler errors are logged, not surfaced.
3. **Synchronous, before the call returns** — a `StreamWrapperMessage`
   with `event.type: "delete"` is broadcast to every WebSocket
   subscribed to the stream and group:
   ```json
   {
     "type":       "stream",
     "timestamp":  1716220800000,
     "streamName": "presence",
     "groupId":    "room-1",
     "id":         "user-123",
     "event":      { "type": "delete", "data": { "online": true, "name": "Alice" } }
   }
   ```
   `event.data` is the value that was removed (the same `old_value`
   from the response).

When the item didn't exist, **none** of these effects happen — no
adapter delete, no spawned trigger task, no WebSocket broadcast.

The actual call-site sequence is **remove → spawn trigger task →
broadcast → return** — the trigger task is the only thing that can
still be running when `delete` returns.

# Worked example

Remove Alice from `room-1` when she logs out:

```json
{
  "stream_name": "presence",
  "group_id":    "room-1",
  "item_id":     "user-123"
}
```

The response carries the removed value as `old_value`, or `{"old_value": null}` when the item didn't exist. The null case is a silent no-op — no adapter delete, no broadcast, no trigger fire.

# Related

- `stream::set` — overwrite with a sentinel instead of removing, when listing endpoints should still see the item.
- `stream::get` — confirm the item is gone after the call (or capture the body before deleting).
- `stream::list` — verify the group's contents after the delete.
- `stream` trigger (`iii://iii-stream/stream/reactive-triggers`) — register a handler that fires on every delete whose filter matches; receives the removed value in `event.data`.
