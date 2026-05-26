---
name: iii-channels
description: >-
  Binary streaming between workers via channels. Use when building data
  pipelines, file transfers, streaming responses, or any pattern requiring
  binary data transfer between functions.
---

# Channels

Comparable to: Unix pipes, gRPC streaming, WebSocket data streams

## Key Concepts

Use the concepts below when they fit the task. Not every worker needs channels.

- A **Channel** is a WebSocket-backed binary stream between two endpoints (writer and reader)
- `createChannel()` returns a writer/reader pair plus serializable refs that can be passed to other workers
- **StreamChannelRef** is a serializable reference (channel_id, access_key, direction) that can be included in function payloads
- Writers send binary data and text messages; Node chunks binary writes into 64KB frames
- Readers consume binary chunks incrementally or via `readAll()`, and receive text messages via callbacks
- Consumers must construct a reader from a serializable `StreamChannelRef` (e.g., `ChannelReader::new(...)`) rather than using the producer-side reader object returned by `createChannel()`
- Channels work cross-worker and cross-language — a Python writer can stream to a Rust reader

## Architecture

A function creates a channel via `createChannel()`, receiving a writer and reader pair. The writer ref or reader ref is passed to another function (potentially in a different worker/language) via a trigger payload. The engine brokers the WebSocket connection between the two endpoints. Binary data flows directly between workers through the engine's channel endpoint.

## iii Primitives Used

| Primitive                              | Purpose                                          |
| -------------------------------------- | ------------------------------------------------ |
| `createChannel(bufferSize?)`           | Create a channel, returns writer + reader pair   |
| `ChannelWriter.write(data)`            | Send binary data (chunked into 64KB frames)      |
| `ChannelWriter.sendMessage(msg)`       | Send a text message through the channel          |
| `ChannelWriter.close()`               | Close the writer end                             |
| `ChannelReader.readAll()`              | Read entire stream into a single buffer          |
| `ChannelReader.onMessage(callback)`    | Register callback for text messages              |
| `StreamChannelRef`                     | Serializable reference to pass between workers   |

## Language-Specific APIs

- Node: `channel.writer.stream.write(buffer)`, `channel.writer.sendMessage(text)`, `channel.reader.stream`, `channel.reader.readAll()`, `channel.reader.onMessage(callback)`.
- Python sync: `iii.create_channel()`, `writer.write(bytes)`, `writer.send_message(text)`, `reader.read_all()`, `reader.on_message(callback)`.
- Python async: `await iii.create_channel_async()`, `await writer.send_message_async(text)`, async iteration over `reader`.
- Rust: `iii.create_channel(None).await`, `ChannelReader::new(engine_ws_base, &reader_ref)`, `ChannelWriter::new(engine_ws_base, &writer_ref)`, `next_binary()`, `read_all()`, `extract_channel_refs(&value)`.
- Browser: `createChannel()`, `writer.sendBinary(uint8Array)`, `writer.sendMessage(text)`, `reader.onBinary(callback)`, `reader.onMessage(callback)`, `reader.readAll()`.

## Reference Implementation

- **TypeScript**: [../references/channels.js](../references/channels.js)
- **Python**: [../references/channels.py](../references/channels.py)
- **Rust**: [../references/channels.rs](../references/channels.rs)

Each reference shows the same patterns (channel creation, binary streaming, text messages, cross-function handoff) in its respective language.
For a shorter copyable channel handoff example, see [iii-examples: Channel Handoff](../iii-examples/SKILL.md#channel-handoff).

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `const channel = await iii.createChannel()` — create a channel pair (producer access)
- `channel.writer.stream.write(buffer)` / `channel.writer.write(data)` — send binary data
- `channel.writer.sendMessage(JSON.stringify({ type: 'metadata', ... }))` — send text metadata
- `channel.writer.close()` — signal end of stream
- Pass `channel.readerRef` or `channel.writerRef` in trigger payloads for cross-worker streaming
- Consumer must reconstruct the reader or writer from the ref when the SDK requires it: e.g., Rust `ChannelReader::new(engine_ws_base, &reader_ref)`
- `const data = await reader.readAll()` — read entire stream (consumer behavior)
- `reader.onMessage(msg => { ... })` — handle text messages (consumer behavior)

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Use channels for large data transfers that shouldn't be serialized into JSON payloads
- Pass `readerRef` to a processing function and `writerRef` to a producing function for pipeline patterns
- Use text messages for metadata/signaling alongside binary data streams
- Set `bufferSize` when the reader may be slower than the writer to apply backpressure
- Channels work cross-language — a TypeScript producer can stream to a Rust consumer

## Pattern Boundaries

- For key-value state persistence, prefer `iii-state-management`.
- For stream CRUD (named streams with groups/keys), prefer `iii-realtime-streams`.
- For pub/sub messaging, prefer triggers with `subscribe` type.
- For compact copyable examples across primitives, prefer `iii-examples`.
- Stay with `iii-channels` when the primary problem is binary data streaming between workers.
- Do not use this skill for removed service APIs or adapter-extension APIs.

## When to Use

- Use this skill when the task is primarily about `iii-channels` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
