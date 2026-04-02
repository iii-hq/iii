[![iii](https://iii.dev/og-image.png)](https://iii.dev)

This is an SDK for [iii](https://iii.dev) — unreasonably simple backend development.

# iii-browser-sdk

Browser SDK for the [iii engine](https://github.com/iii-hq/iii) — WebSocket-based, no Node.js dependencies, no OpenTelemetry.

[![npm](https://img.shields.io/npm/v/iii-browser-sdk)](https://www.npmjs.com/package/iii-browser-sdk)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](../../../LICENSE)

## Why the browser SDK

The browser SDK turns your frontend into a **first-class iii worker**:

- **Persistent connection** — one WebSocket replaces many HTTP round-trips.
- **Bi-directional** — the engine can invoke functions registered in the browser. Backend workers push data to the frontend with `trigger()`, enabling real-time patterns without polling.
- **Same API** — `registerFunction`, `trigger`, `registerTrigger`, `onFunctionsAvailable` — all the primitives you use server-side work identically in the browser.
- **Zero Node.js dependencies** — runs in any browser environment with native `WebSocket`.

## Install

```bash
npm install iii-browser-sdk
```

## Hello World

```typescript
import { registerWorker } from 'iii-browser-sdk'

const iii = registerWorker('ws://localhost:49135')

iii.registerFunction(
  { id: 'ui::show-notification' },
  async (data: { title: string; body: string }) => {
    showToast(data.title, data.body)
    return { displayed: true }
  },
)

const users = await iii.trigger({
  function_id: 'api::get::users',
  payload: {},
})
```

## API

| Operation                | Signature                                                             | Description                                                  |
| ------------------------ | --------------------------------------------------------------------- | ------------------------------------------------------------ |
| Initialize               | `registerWorker(url, options?)`                                       | Connect to the engine via browser WebSocket. Returns `ISdk`  |
| Register function        | `iii.registerFunction({ id }, handler)`                               | Register a function the engine (or backend) can invoke       |
| Register trigger         | `iii.registerTrigger({ type, function_id, config })`                  | Bind a trigger to a function                                 |
| Invoke (await)           | `await iii.trigger({ function_id, payload })`                         | Invoke a function and wait for the result                    |
| Invoke (fire-and-forget) | `iii.trigger({ function_id, payload, action: TriggerAction.Void() })` | Invoke without waiting                                       |
| Create channel           | `iii.createChannel()`                                                 | Create a streaming channel pair (writer + reader)            |
| Functions available      | `iii.onFunctionsAvailable(fn)`                                        | Subscribe to function availability changes                   |
| Shutdown                 | `iii.shutdown()`                                                      | Gracefully disconnect from the engine                        |

### Registering Functions

Register a function in the browser that backend workers can call:

```typescript
iii.registerFunction(
  { id: 'ui::show-notification' },
  async (data: { title: string; body: string }) => {
    showToast(data.title, data.body)
    return { displayed: true }
  },
)
```

### Calling Backend Functions

Invoke any function registered in the engine directly from the browser:

```typescript
const users = await iii.trigger({
  function_id: 'api::get::users',
  payload: {},
})
```

### Receiving Live Invocations

Backend workers can push data to the browser in real time. No polling required:

```typescript
iii.registerFunction(
  { id: 'ui::update-dashboard' },
  async (metrics: { cpu: number; memory: number; requests: number }) => {
    document.getElementById('cpu')!.textContent = `${metrics.cpu}%`
    document.getElementById('memory')!.textContent = `${metrics.memory}MB`
    document.getElementById('requests')!.textContent = `${metrics.requests}/s`
    return null
  },
)
```

## Exports

| Import                   | What it provides                      |
| ------------------------ | ------------------------------------- |
| `iii-browser-sdk`        | Core SDK (`registerWorker`, types)    |
| `iii-browser-sdk/stream` | Stream types for real-time state      |
| `iii-browser-sdk/state`  | State types for key-value operations  |

## Resources

- [Documentation](https://iii.dev/docs)
- [Use iii in the Browser](https://iii.dev/docs/how-to/use-iii-in-the-browser)
- [Browser SDK API Reference](https://iii.dev/docs/api-reference/sdk-browser)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)
