---
name: realtime-streams
description: >-
  Pushes live updates to connected WebSocket clients via streams. Use when
  building real-time dashboards, live feeds, or collaborative features.
---

# Realtime Streams

Comparable to: Socket.io, Pusher, Firebase Realtime

## Key Concepts

Use the concepts below when they fit the task. Not every stream setup needs all of them.

- **StreamModule** serves WebSocket connections on port 3112
- Clients connect at `ws://host:3112/stream/{stream_name}/{group_id}`
- **stream::set** / **stream::get** / **stream::list** / **stream::delete** provide CRUD for stream items
- **stream::send** pushes events to all connected clients in a stream group
- `createStream` registers a custom adapter for non-default stream backends
- Each stream item is addressed by `stream_name`, `group_id`, `item_id`, and `data`

## Architecture

    Function
      → trigger('stream::set', { stream_name, group_id, item_id, data })
      → trigger('stream::send', { stream_name, group_id, data })
        → StreamModule
          → WebSocket push
            → Connected clients at /stream/{stream_name}/{group_id}

## iii Primitives Used

| Primitive                                                                | Purpose                                     |
| ------------------------------------------------------------------------ | ------------------------------------------- |
| `trigger({ function_id: 'stream::set', payload })`                       | Create or update a stream item              |
| `trigger({ function_id: 'stream::get', payload })`                       | Read a stream item                          |
| `trigger({ function_id: 'stream::list', payload })`                      | List items in a stream group                |
| `trigger({ function_id: 'stream::delete', payload })`                    | Remove a stream item                        |
| `trigger({ function_id: 'stream::send', payload })`                      | Push an event to connected clients          |
| `createStream`                                                           | Register a custom stream adapter            |

## Reference Implementation

See [references/reference.js](references/reference.js) for the full working example — a stream that pushes live updates to WebSocket clients and manages stream items with CRUD operations.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `trigger({ function_id: 'stream::set', payload: { stream_name, group_id, item_id, data } })` — write stream item
- `trigger({ function_id: 'stream::send', payload: { stream_name, group_id, data } })` — push event to clients
- `trigger({ function_id: 'stream::get', payload: { stream_name, group_id, item_id } })` — read stream item
- `trigger({ function_id: 'stream::list', payload: { stream_name, group_id } })` — list items in group
- `createStream(name, adapter)` — custom adapter for specialized backends
- `const logger = new Logger()` — structured logging

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Name streams after your domain (e.g. `chat-messages`, `dashboard-metrics`, `notifications`)
- Use `group_id` to partition streams per user, room, or tenant
- Combine with `state-reactions` to push a stream event whenever state changes
- Use `createStream` when the default adapter does not fit (e.g. custom persistence or fan-out logic)

## Pattern Boundaries

- If the task is about persistent key-value data without real-time push, prefer `state-management`.
- If the task needs reactive triggers on state changes (server-side), prefer `state-reactions`.
- Stay with `realtime-streams` when the primary need is pushing live updates to connected clients.
