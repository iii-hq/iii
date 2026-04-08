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

| Export                                                   | Purpose                                           |
| -------------------------------------------------------- | ------------------------------------------------- |
| `registerWorker(address, options?)`                      | Connect to the engine via WebSocket               |
| `registerFunction(id, handler)`                          | Register a browser-side function handler          |
| `registerTrigger({ type, function_id, config, metadata? })` | Bind a trigger to a function                  |
| `trigger({ function_id, payload, action? })`             | Invoke a function                                 |
| `TriggerAction.Void()`                                   | Fire-and-forget invocation mode                   |
| `TriggerAction.Enqueue({ queue })`                       | Durable async invocation mode                     |
| `registerTriggerType({ id, description }, { registerTrigger, unregisterTrigger })` | Custom trigger type registration |
| `createChannel()`                                        | Binary streaming between workers                  |

## Key Differences from Node SDK

- No custom WebSocket headers — uses query parameters for auth tokens
- No `Logger` export — use browser console or your own logging
- No worker metadata telemetry reporting
- Connects directly via `ws://` or `wss://` URL (no `registerWorker` URL options)
- Same function/trigger/channel API surface as the Node SDK

## Quick Start

```typescript
import { registerWorker, TriggerAction } from 'iii-browser-sdk'

const iii = registerWorker('ws://localhost:49135')

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

- `registerWorker('ws://host:49135')` — connect from browser
- `registerWorker('wss://host:49135')` — connect with TLS in production
- `iii.registerFunction(id, handler)` — register browser-side handler
- `iii.trigger({ function_id, payload })` — call server-side functions
- `iii.trigger({ ..., action: TriggerAction.Void() })` — fire-and-forget from browser
- Stream connections at `ws://host:3112/stream/{name}/{group}` for real-time updates

## Pattern Boundaries

- For server-side Node.js workers, prefer `iii-node-sdk`.
- For real-time stream consumption patterns, see `iii-realtime-streams`.
- For Python or Rust workers, see `iii-python-sdk` or `iii-rust-sdk`.
- Stay with `iii-browser-sdk` when the client is a web browser.

## When to Use

- Use this skill when the task is primarily about `iii-browser-sdk` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
