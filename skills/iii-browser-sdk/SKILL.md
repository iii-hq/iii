---
name: iii-browser-sdk
description: >-
  Browser SDK for connecting to the iii engine from web applications via
  WebSocket. Use when building browser-based clients that register functions,
  invoke triggers, or consume streams from the frontend.
---

# Browser SDK

The browser-optimized SDK for connecting web applications to the iii engine.

## Documentation

Full API reference: <https://iii.dev/docs/api-reference/sdk-browser>

## Install

`npm install iii-browser-sdk`

## Key Exports

| Export                                      | Purpose                                  |
| ------------------------------------------- | ---------------------------------------- |
| `registerWorker(address, options?)`         | Connect to the engine via WebSocket      |
| `registerFunction(id, handler, options?)`   | Register a browser-side function handler |
| `registerTrigger({ type, function_id, config })` | Bind a trigger to a function        |
| `trigger({ function_id, payload, action?, timeoutMs? })` | Invoke a function              |
| `TriggerAction.Void()`                      | Fire-and-forget invocation mode          |
| `TriggerAction.Enqueue({ queue })`          | Durable async invocation mode            |
| `registerTriggerType({ id, description }, handler)` | Custom trigger type registration |
| `createChannel()`                           | Binary/text streaming between workers    |
| `addConnectionStateListener(handler)`        | Observe connect/disconnect state         |

## Key Differences from Node SDK

- No custom WebSocket headers — use query parameters, cookies, or a protected RBAC listener for auth
- No `Logger` export — use browser console or your own logging
- No SDK OpenTelemetry export from the browser package
- No HTTP-invoked function registration from the browser package
- Trigger metadata is not available in the current browser registration API
- Connects directly via `ws://` or `wss://` URL (no `registerWorker` URL options)
- Function, trigger, and channel APIs are similar to Node, but not identical

## Security Model

- Do not expose the internal engine worker port directly to untrusted browsers.
- Connect browser apps to an RBAC-protected listener, usually a dedicated `iii-worker-manager` port.
- Put auth context in query params, cookies, or an upstream session; custom WebSocket headers are not portable in browsers.
- Keep secrets, API keys, and service credentials on server-side workers.
- Browser functions are useful for real-time UI callbacks and interactive tools, not privileged backend work.

## Quick Start

```typescript
import { registerWorker, TriggerAction } from 'iii-browser-sdk'

const iii = registerWorker('ws://localhost:49135?token=dev-token')

iii.registerFunction('ui::greet', async (data) => {
  return { message: `Hello, ${data.name}!` }
})

const result = await iii.trigger({
  function_id: 'backend::get-user',
  payload: { userId: '123' },
})

await iii.trigger({
  function_id: 'analytics::track',
  payload: { event: 'page_view' },
  action: TriggerAction.Void(),
})
```

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker('ws://host:49135?token=...')` — connect from browser to an RBAC listener
- `registerWorker('wss://host:49135?token=...')` — connect with TLS in production
- `iii.registerFunction(id, handler)` — register browser-side handler
- `iii.trigger({ function_id, payload })` — call server-side functions
- `iii.trigger({ ..., action: TriggerAction.Void() })` — fire-and-forget from browser
- `const { writer, reader, readerRef, writerRef } = await iii.createChannel()` — create a channel
- `writer.sendBinary(uint8Array)` / `writer.sendMessage(text)` — send channel data
- `reader.onBinary(callback)` / `reader.onMessage(callback)` / `reader.readAll()` — consume channel data
- `iii.addConnectionStateListener(listener)` — update UI on reconnects

## Pattern Boundaries

- For server-side Node.js workers, prefer `iii-node-sdk`.
- For channel-based binary transfer, see `iii-channels`.
- Configure RBAC-protected browser access on the worker-manager listener in engine config.
- For Python or Rust workers, see `iii-python-sdk` or `iii-rust-sdk`.
- Stay with `iii-browser-sdk` when the client is a web browser.

## When to Use

- Use this skill when the task is primarily about `iii-browser-sdk` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
