---
type: how-to
function_id: stream::set
title: Persist an item and broadcast a create/update event
---

# When to use

Call `stream::set` when you have the full `(stream_name, group_id,
item_id)` triple and want to overwrite the value at that location, then
notify every WebSocket subscriber and every registered `stream::*`
trigger in one step. The response carries both the previous and the new
value, so callers can branch on insert vs. overwrite without re-reading.

Reach for it when:

- You are writing the canonical state for an item (a presence record, a
  chat message, a job's current status) and want connected UIs to update
  in real time without a poll.
- You need to know whether the write was a create or an update —
  `old_value: null` means the item didn't exist before.
- A trigger registered against this `(stream, group, item)` should fire
  as part of the same call.

Use [`stream::update`](iii://iii-stream/stream/update) instead when concurrent
writers can race on the same item — `update` applies an op list
atomically against the current value, where `set` blindly overwrites.

Use [`stream::send`](iii://iii-stream/stream/send) instead when you want to
broadcast a transient event (typing indicator, cursor blip) that
**should not** be stored or replay on reconnect.

# Inputs

```json
{
  "stream_name": "presence",                   // required; top-level namespace
  "group_id":    "room-1",                     // required; second-level partition (e.g. one room, one user, one tenant)
  "item_id":     "user-123",                   // required; identifier of the item within the group
  "data":        { "online": true, "name": "Alice" }   // required; arbitrary JSON value stored at this location
}
```

All four fields are required. `data` accepts any JSON value (object,
array, string, number, boolean, `null`); the worker stores it as-is and
hands it back unchanged on subsequent reads.

# Outputs

```json
{
  "old_value": { "online": false, "name": "Alice" },   // value before the write; null if the item didn't exist
  "new_value": { "online": true,  "name": "Alice" }    // value after the write; mirrors the request `data`
}
```

- `old_value` is `null` exactly when the item did not exist before the
  call. Use `old_value === null` as the create/update signal rather than
  comparing payloads.
- `new_value` always equals the `data` you sent. It's echoed back so
  callers can pipe the response straight into UI state without
  reconstructing the payload.
- On a custom override (registering a function at
  `stream::set(<stream_name>)`), the override's return value replaces
  this envelope verbatim — keep the same shape.

# Side effects

A successful `set` produces three observable effects:

1. **Synchronous, before the call returns** — the adapter persists
   `data` at `(stream_name, group_id, item_id)`. A subsequent
   `stream::get` from any caller (including from inside a trigger
   handler) sees the new value.
2. **Asynchronous, fire-and-forget** — the worker spawns a task that
   invokes every registered [`stream`](iii://iii-stream/stream/reactive-triggers)
   trigger whose `(stream_name, group_id, item_id)` filter matches.
   The spawn happens before the call returns, but the handlers run on
   that spawned task **after** the original `set` call completes.
   A slow or failing trigger handler does not delay or fail the
   originating `set`. Per-handler errors are logged, not surfaced.
3. **Synchronous, before the call returns** — a `StreamWrapperMessage`
   is broadcast to every WebSocket subscribed to the stream and group.
   Shape:
   ```json
   {
     "type":       "stream",
     "timestamp":  1716220800000,                 // milliseconds since epoch
     "streamName": "presence",
     "groupId":    "room-1",
     "id":         "user-123",
     "event":      { "type": "create", "data": { "online": true, "name": "Alice" } }
   }
   ```
   `event.type` is `"create"` when `old_value` was `null` and
   `"update"` otherwise. `event.data` always carries the new value.

The actual call-site sequence is **persist → spawn trigger task →
broadcast → return** — the trigger task is the only thing that can
still be running when `set` returns.

# Worked example

Mark Alice as online in `room-1`:

```json
{
  "stream_name": "presence",
  "group_id":    "room-1",
  "item_id":     "user-123",
  "data":        { "online": true, "name": "Alice", "joined_at": "2026-05-20T17:00:00Z" }
}
```

The response carries the prior value at that location, or `null` when the item is brand new — branch on `old_value === null` to distinguish create from update without a follow-up `stream::get`.

# Related

- `stream::update` — atomically apply ops to the existing value instead of overwriting; safe for concurrent writers.
- `stream::get` — read the same item back; needed when you don't already have the value in hand.
- `stream::delete` — remove the item; broadcasts a `delete` event with the same `(stream, group, item)` shape.
- `stream::send` — broadcast a transient event without persisting; counterpart for ephemeral signals.
- `stream` trigger (`iii://iii-stream/stream/reactive-triggers`) — register a handler that fires on every successful `set` whose filter matches.
