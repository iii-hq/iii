---
type: how-to
function_id: stream::send
title: Broadcast a transient event to a group without persisting
---

# When to use

Call `stream::send` when you want every subscriber of a `(stream_name,
group_id)` pair to receive a custom event **without** changing any
stored item. The event is delivered over the same WebSocket fan-out
that `stream::set` uses, but no adapter write happens — clients that
connect later will not see this event on replay.

Reach for it when:

- You are signalling something ephemeral (typing indicator, cursor
  position, remote-cursor blink, "user is dragging") that loses its
  meaning the moment a new one arrives.
- You want to trigger an animation or toast on every connected client
  but don't want a stale entry sitting in the store.
- You need to target a single subscriber inside a group via the
  optional `id` field rather than the whole group.

Use [`stream::set`](iii://iii-stream/stream/set) instead when the event represents
durable state that late subscribers need to see (a chat message, a job
status, a cart item). `set` persists; `send` does not.

| Question                                            | Use this                       |
|-----------------------------------------------------|--------------------------------|
| Should this event survive a page reload?            | `stream::set` (or `update`)    |
| Should the event vanish the moment a new one fires? | `stream::send`                 |
| Does a UI table or list need this?                  | `stream::set` (or `update`)    |
| Is this "user X is typing in room Y right now"?     | `stream::send`                 |

# Inputs

```json
{
  "stream_name": "presence",                   // required; top-level namespace
  "group_id":    "room-1",                     // required; second-level partition (the broadcast scope)
  "id":          "user-456",                   // optional; subscriber id to single-cast within the group, omit for whole-group broadcast
  "type":        "typing",                     // required; event type label, surfaced to clients as `event.event.type`
  "data":        { "user_id": "user-123" }     // required; arbitrary JSON payload, surfaced as `event.event.data`
}
```

`stream_name`, `group_id`, `type`, and `data` are required. `id` is
optional — when present, the broadcast is scoped to the subscriber that
joined with that `id`; when omitted, every subscriber of the
`(stream_name, group_id)` pair receives the event.

Note the field name differs from `stream::set`: `send` uses `type` (the
event-type label), where `set` uses `data` directly. The `type` field
is what clients read as `event.event.type` to dispatch.

# Outputs

```json
null
```

- The function returns `null` on success — there's no envelope, no echo
  of the inputs.
- Failures surface as a `STREAM_SEND_ERROR` in the standard
  `FunctionResult::Failure` shape, carrying the underlying adapter
  error message.

# Side effects

A successful `send` produces two effects:

1. **Asynchronous, fire-and-forget** — the worker spawns a task that
   invokes every registered [`stream`](iii://iii-stream/stream/reactive-triggers)
   trigger whose filter matches. Yes — `stream::send` fires `stream`
   triggers even though it doesn't persist anything. Handlers run on
   that spawned task **after** the `send` call returns. Trigger
   handlers should inspect `event.event.type === "event"` to
   distinguish `send` payloads from `set`/`update`/`delete` payloads.
2. **Synchronous, before the call returns** — a `StreamWrapperMessage`
   with `event.type: "event"` is broadcast to every WebSocket
   subscribed to the stream and group (or the single subscriber
   identified by `id`, when set). Shape:
   ```json
   {
     "type":       "stream",
     "timestamp":  1716220800000,
     "streamName": "presence",
     "groupId":    "room-1",
     "id":         "user-456",
     "event":      {
       "type":  "event",
       "event": { "type": "typing", "data": { "user_id": "user-123" } }
     }
   }
   ```
   Note the nesting: the wrapper's `event.type` is always the literal
   `"event"`, and the user-supplied `type` from the request lives at
   `event.event.type`.

There is **no adapter write**. `stream::get` and `stream::list` are
unchanged after a `send`. The actual call-site sequence is **spawn
trigger task → broadcast → return**.

# Worked example

Broadcast a "typing" indicator to everyone in `room-1`:

```json
{
  "stream_name": "presence",
  "group_id":    "room-1",
  "type":        "typing",
  "data":        { "user_id": "user-123" }
}
```

To target a single subscriber inside the group rather than the whole group, add an `id` matching the subscriber id from the `stream:join` event. The function returns `null` on success — see **Outputs** for the broadcast envelope shape.

# Related

- `stream::set` — persistent counterpart; use when late subscribers need to see the value on reconnect.
- `stream::update` — atomic mutation of a stored item; counterpart to `set` for race-prone writes.
- `stream::delete` — remove a stored item; `send` does not delete or modify anything.
- `stream` trigger (`iii://iii-stream/stream/reactive-triggers`) — register a handler that fires on every `send`; differentiate from set/update/delete payloads via `event.event.type === "event"`.
