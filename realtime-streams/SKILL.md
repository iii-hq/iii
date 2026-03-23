---
name: realtime-streams
description: >-
  Push live updates to WebSocket clients via iii streams. Use when building
  chat, dashboards, notifications, or any feature requiring real-time data
  delivery. Comparable to Socket.io, Pusher, or Firebase Realtime Database.
---

# Realtime Streams

Push data to connected WebSocket clients through named streams.

## Key Concepts

- A **Stream** is a named channel that clients subscribe to via WebSocket
- Data is organized by `stream_name` and `group_id`, with individual `item_id` entries
- Clients connect at `ws://host:3112/stream/{stream_name}/{group_id}` and receive updates automatically
- Storage is handled by configurable adapters (file-based, in-memory, or Redis)
- The **StreamModule** runs on a dedicated port (default 3112) configured in `iii-config.yaml`

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `stream::set` | Write data and push to connected clients |
| `stream::get` | Read a single item from a stream |
| `stream::list` | List all items in a stream group |
| `stream::delete` | Remove an item from a stream |

## Common Patterns

- `trigger({ function_id: 'stream::set', payload: { stream_name, group_id, item_id, data } })` -- write and broadcast
- `trigger({ function_id: 'stream::list', payload: { stream_name, group_id } })` -- read all items in a group
- `trigger({ function_id: 'stream::get', payload: { stream_name, group_id, item_id } })` -- read a single item
- `trigger({ function_id: 'stream::delete', payload: { stream_name, group_id, item_id } })` -- remove an item
- Client: `new WebSocket('ws://localhost:3112/stream/chat/room-123')` -- connect and listen

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with configuration, adapters, and TypeScript/Python/Rust examples.

## Pattern Boundaries

- For function registration and trigger binding, see `functions-and-triggers`
- For state persistence (key-value, not streaming), see `state-management`
- For durable message processing, see `queue-processing`
