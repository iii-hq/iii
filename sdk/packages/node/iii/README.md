# iii-sdk

Node.js / TypeScript SDK for the [iii engine](https://github.com/iii-hq/iii).

[![npm](https://img.shields.io/npm/v/iii-sdk)](https://www.npmjs.com/package/iii-sdk)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](../../../LICENSE)

## Install

```bash
pnpm add iii-sdk
# or: npm install iii-sdk
```

## Hello World

```javascript
import { registerWorker } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

iii.registerFunction('hello::greet', async (input) => {
  return { message: `Hello, ${input.name}!` }
})

const result = await iii.trigger({ function_id: 'hello::greet', payload: { name: 'world' } })
```

## API

| Operation                | Signature                                                                         | Description                                                    |
| ------------------------ | --------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| Initialize               | `registerWorker(url, options?)`                                                   | Create and connect to the engine. Returns an `IIIClient`       |
| Register function        | `iii.registerFunction(id, handler, options?)`                                     | Register a function that can be invoked by name                |
| Invoke (await)           | `await iii.trigger({ function_id, payload })`                                     | Invoke a function and wait for the result                      |
| Invoke (fire-and-forget) | `iii.trigger({ function_id, payload, action: TriggerAction.Void() })`             | Invoke without waiting                                         |
| Invoke (enqueue)         | `iii.trigger({ function_id, payload, action: TriggerAction.Enqueue({ queue }) })` | Route invocation through a named queue                         |
| Observe                  | `iii.observe({ function_id, args?, filters? }, target, options?)`                 | Invoke `target` on every matching invocation of a function     |
| Current invocation       | `iii.currentInvocation()`                                                         | The invocation the running handler serves (caller, activation) |

### Registering Functions

```javascript
iii.registerFunction('orders::create', async (input) => {
  return { status_code: 201, body: { id: '123', item: input.body.item } }
})
```

### Invoking Functions

```javascript
import { registerWorker, TriggerAction } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

const result = await iii.trigger({ function_id: 'orders::create', payload: { item: 'widget' } })

iii.trigger({ function_id: 'analytics::track', payload: { event: 'page_view' }, action: TriggerAction.Void() })
```

A call to a function this worker registered itself runs in-process once the
engine confirmed it (no frames on the wire unless someone observes it). Pass
`dispatch: 'remote'` to go through the engine anyway, or register the function
with `localDispatch: false`.

### Observing Functions

Every function is observable. The target is a registered function; it receives
the `Observed` envelope (`input`, `output` or `error`, `caller`, ...).

```javascript
iii.registerFunction('alerts::big-order', async ({ input, output }) => notify(input, output))

const observer = iii.observe(
  { function_id: 'orders::create', filters: { 'input.body.total': { $gte: 500 } } },
  'alerts::big-order',
)
await observer.ready()
// later
await observer.dispose()
```

A function can hand its own events to the observers they belong to (owner
delivery), and react when one is added or removed:

```javascript
const schedules = new Map() // expression -> observers

iii.registerFunction('cron::on-tick', async (tick) => {
  for (const obs of schedules.get(tick.expression) ?? []) obs.emit(tick)
}, {
  observe: {
    delivery: 'owner',
    args_format: { type: 'object', required: ['expression'], properties: { expression: { type: 'string' } } },
    onObserverAdded(obs) {
      if (!schedules.has(obs.args.expression)) schedules.set(obs.args.expression, new Set())
      schedules.get(obs.args.expression).add(obs)
    },
    onObserverRemoved(obs) {
      schedules.get(obs.args.expression)?.delete(obs)
    },
  },
})
```

### Trigger APIs (deprecated)

`registerTrigger` and `registerTriggerType` keep working in 0.25 but are
deprecated: they log a warning once per process. Use `iii.observe(source,
target)` and `registerFunction(id, handler, { observe })` instead. See the
[migration guide](https://iii.dev/docs/upgrading/from-0-24-x).

## Resources

- [Documentation](https://iii.dev/docs)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)
