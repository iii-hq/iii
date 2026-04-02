# iii-sdk

Node.js / TypeScript SDK for the [iii engine](https://github.com/iii-hq/iii).

[![npm](https://img.shields.io/npm/v/iii-sdk)](https://www.npmjs.com/package/iii-sdk)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](../../../LICENSE)

## Install

```bash
npm install iii-sdk
```

## Hello World

```javascript
import { registerWorker } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

iii.registerFunction({ id: 'greet' }, async (input) => {
  return { message: `Hello, ${input.name}!` }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'greet',
  config: { api_path: '/greet', http_method: 'POST' },
})

const result = await iii.trigger({ function_id: 'greet', payload: { name: 'world' } })
```

## API

| Operation                | Signature                                            | Description                                                  |
| ------------------------ | ---------------------------------------------------- | ------------------------------------------------------------ |
| Initialize               | `registerWorker(url, options?)`                      | Create and connect to the engine. Returns an `ISdk` instance |
| Register function        | `iii.registerFunction({ id }, handler)`              | Register a function that can be invoked by name              |
| Register trigger         | `iii.registerTrigger({ type, function_id, config })` | Bind a trigger (HTTP, cron, queue, etc.) to a function       |
| Invoke (await)           | `await iii.trigger({ function_id, payload })`       | Invoke a function and wait for the result                    |
| Invoke (fire-and-forget) | `iii.trigger({ function_id, payload, action: TriggerAction.Void() })` | Invoke without waiting |

### Registering Functions

```javascript
iii.registerFunction({ id: 'orders.create' }, async (input) => {
  return { status_code: 201, body: { id: '123', item: input.body.item } }
})
```

### Registering Triggers

```javascript
iii.registerTrigger({
  type: 'http',
  function_id: 'orders.create',
  config: { api_path: '/orders', http_method: 'POST' },
})
```

### Invoking Functions

```javascript
import { registerWorker, TriggerAction } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

const result = await iii.trigger({ function_id: 'orders.create', payload: { item: 'widget' } })

iii.trigger({ function_id: 'analytics.track', payload: { event: 'page_view' }, action: TriggerAction.Void() })
```

## Node Modules

| Import              | What it provides                      |
| ------------------- | ------------------------------------- |
| `iii-sdk`           | Core SDK (`registerWorker`, types)    |
| `iii-sdk/stream`    | Stream client for real-time state     |
| `iii-sdk/state`     | State client for key-value operations |
| `iii-sdk/telemetry` | OpenTelemetry integration             |

## Removed methods

`call`, `callVoid`, and `triggerVoid` have been removed. Use `trigger()` for all invocations. For fire-and-forget, use `trigger({ function_id, payload, action: TriggerAction.Void() })`.

## Resources

- [Documentation](https://iii.dev/docs)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)
