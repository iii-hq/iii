---
name: iii-realtime-streams
description: >-
  Pushes live updates to connected WebSocket clients via streams. Use when
  building real-time dashboards, live feeds, or collaborative features.
---

# Realtime Streams

Comparable to: Socket.io, Pusher, Firebase Realtime

## Key Concepts

Use the concepts below when they fit the task. Not every stream setup needs all of them.

- **iii-stream** serves WebSocket connections on the configured stream port (default 3112)
- Install or enable streams with `iii worker add iii-stream`
- Clients connect at `ws://host:{stream_port}/stream/{stream_name}/{group_id}`
- **stream::set** / **stream::update** / **stream::get** / **stream::list** / **stream::delete** provide CRUD for stream items
- **stream::send** pushes events to all connected clients in a stream group
- Each stream item is identified by `stream_name`, `group_id`, and `item_id`; `data` is the item payload
- Trigger types `stream`, `stream:join`, and `stream:leave` let workers react to data changes and WebSocket subscriptions

## Architecture

    Function
      → trigger('stream::set', { stream_name, group_id, item_id, data })
      → trigger('stream::send', { stream_name, group_id, data })
        → iii-stream
          → WebSocket push
            → Connected clients at /stream/{stream_name}/{group_id}

## iii Primitives Used

| Primitive                                             | Purpose                            |
| ----------------------------------------------------- | ---------------------------------- |
| `trigger({ function_id: 'stream::set', payload })`    | Create or update a stream item     |
| `trigger({ function_id: 'stream::update', payload })` | Atomically update a stream item    |
| `trigger({ function_id: 'stream::get', payload })`    | Read a stream item                 |
| `trigger({ function_id: 'stream::list', payload })`   | List items in a stream group       |
| `trigger({ function_id: 'stream::list_groups', payload })` | List groups in a stream       |
| `trigger({ function_id: 'stream::list_all', payload })` | List streams with group metadata   |
| `trigger({ function_id: 'stream::delete', payload })` | Remove a stream item               |
| `trigger({ function_id: 'stream::send', payload })`   | Push an event to connected clients |

## Reference Implementation

See [../references/realtime-streams.js](../references/realtime-streams.js) for the full working example — a stream that pushes live updates to WebSocket clients and manages stream items with CRUD operations.

Also available in **Python**: [../references/realtime-streams.py](../references/realtime-streams.py)

Also available in **Rust**: [../references/realtime-streams.rs](../references/realtime-streams.rs)

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `trigger({ function_id: 'stream::set', payload: { stream_name, group_id, item_id, data } })` — write stream item
- `trigger({ function_id: 'stream::update', payload: { stream_name, group_id, item_id, ops } })` — atomic stream item update
- `trigger({ function_id: 'stream::send', payload: { stream_name, group_id, data } })` — push event to clients
- `trigger({ function_id: 'stream::get', payload: { stream_name, group_id, item_id } })` — read stream item
- `trigger({ function_id: 'stream::list', payload: { stream_name, group_id } })` — list items in group
- `registerTrigger({ type: 'stream', config: { stream_name, group_id, item_id } })` — react to item changes
- `registerTrigger({ type: 'stream:join' })` / `registerTrigger({ type: 'stream:leave' })` — react to client subscriptions

### Browser Clients

For browser-side WebSocket connections, use `iii-browser-sdk` for browser workers or direct stream WebSocket URLs for stream clients. Keep browser access behind the intended public stream or RBAC listener, not the private engine worker port.

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Name streams after your domain (e.g. `chat-messages`, `dashboard-metrics`, `notifications`)
- Use `group_id` to partition streams per user, room, or tenant
- Combine with `iii-state-reactions` to push a stream event whenever state changes
- Use `auth_function` in iii-stream config when browser clients need connection context.

## Engine Configuration

Install/enable iii-stream with `iii worker add iii-stream`. It must be configured with a port and adapter (`kv` for local/file persistence or Redis for multi-instance delivery). See [../references/iii-config.yaml](../references/iii-config.yaml) for the full annotated config reference.

## Pattern Boundaries

- If the task is about persistent key-value data without real-time push, prefer `iii-state-management`.
- If the task needs reactive triggers on state changes (server-side), prefer `iii-state-reactions`.
- Stay with `iii-realtime-streams` when the primary need is pushing live updates to connected clients.
- Do not generate removed service APIs or adapter-extension APIs from this skill.

## When to Use

- Use this skill when the task is primarily about `iii-realtime-streams` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
