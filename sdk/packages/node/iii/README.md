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

const result = await iii.call({ function_id: 'hello::greet', payload: { name: 'world' } })
```

## API

| Operation                | Signature                                                                     | Description                                                     |
| ------------------------ | ----------------------------------------------------------------------------- | --------------------------------------------------------------- |
| Initialize               | `registerWorker(url, options?)`                                               | Create and connect to the engine. Returns an `IIIClient`        |
| Register function        | `iii.registerFunction(id, handler, options?)`                                 | Register a function that can be called by id and observed       |
| Call (await)             | `await iii.call({ function_id, payload })`                                    | Call a function and wait for the result                         |
| Call (fire-and-forget)   | `iii.call({ function_id, payload, action: CallAction.Void() })`               | Call without waiting                                            |
| Call (enqueue)           | `iii.call({ function_id, payload, action: CallAction.Enqueue({ queue }) })`   | Route the call through a named queue                            |
| Observe                  | `iii.observe('fn::id' \| { function_id, args?, filters? }, target, options?)` | Call `target` on every matching call of a function              |
| Observer hooks           | `onObserverAdded`, `onObserverRemoved`, `onObserverDeliver`                   | Change what observing a function this worker hosts means        |
| Call through an observer | `await iii.call({ observer, payload })`                                       | Run an observer's target, with its policy and metadata          |
| Current invocation       | `iii.currentInvocation()`                                                     | The invocation the running handler serves (caller, activation)  |

`iii.trigger` and `TriggerAction` are the same function and object as
`iii.call` and `CallAction`.

### Registering Functions

```javascript
iii.registerFunction('orders::create', async (input) => {
  return { status_code: 201, body: { id: '123', item: input.body.item } }
})
```

The handler's return value is what the function's observers observe.

### Calling Functions

```javascript
import { CallAction, registerWorker } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

const result = await iii.call({ function_id: 'orders::create', payload: { item: 'widget' } })

iii.call({ function_id: 'analytics::track', payload: { event: 'page_view' }, action: CallAction.Void() })
```

A call to a function this worker registered itself runs in-process once the
engine confirmed it (no frames on the wire unless someone observes it). Pass
`dispatch: 'remote'` to go through the engine anyway, or register the function
with `localDispatch: false`.

### Observing Functions

Every function is observable. When `orders::create` runs, its observers run.
The target is a registered function; it receives the `Observed` envelope
(`input`, `output` or `error`, `caller`, ...).

```javascript
iii.registerFunction('alerts::big-order', async ({ input, output }) => notify(input, output))

// every successful call
iii.observe('orders::create', 'search::reindex-order')

// only large orders
const observer = iii.observe(
  { function_id: 'orders::create', filters: { 'input.body.total': { $gte: 500 } } },
  'alerts::big-order',
)
await observer.ready()
// later
await observer.dispose()
```

### Hosting Observable Functions

The worker that hosts a function decides what observing it means with three
hooks, each overriding a default:

| Hook                                       | Default                                                         |
| ------------------------------------------ | --------------------------------------------------------------- |
| `onObserverAdded(obs)`                     | accept the observer                                             |
| `onObserverRemoved(obs)`                   | stop delivering to it and forget it                             |
| `onObserverDeliver(call, observers, next)` | after each call, run every observer whose filters match the call |

`onObserverAdded` and `onObserverRemoved` can be set for every function the
worker hosts, or for one function by id or on its `FunctionRef`.
`onObserverDeliver` is set per function. Each method returns its receiver, and
`null` removes a hook.

An event source returns the event it is called with. `args_format` describes
the `args` its observers pass, and `private: true` lets only this worker call
it:

```javascript
import { keyed, ObserveRejection } from 'iii-sdk'

iii
  .registerFunction('cron::on-tick', (tick) => tick, {
    args_format: { type: 'object', required: ['expression'], properties: { expression: { type: 'string' } } },
    private: true,
  })
  .onObserverAdded((obs) => {
    if (!isValidCron(obs.args.expression)) throw new ObserveRejection('INVALID_CRON_EXPRESSION', 'bad expression')
    scheduler.retain(obs.args.expression, (scheduled_time) =>
      iii.call({ function_id: 'cron::on-tick', payload: { expression: obs.args.expression, scheduled_time } }))
  })
  .onObserverRemoved((obs) => scheduler.release(obs.args.expression))
  // each tick reaches the observers of its expression
  .onObserverDeliver(keyed({ observer: (obs) => obs.args.expression, call: (call) => call.input.expression }))

// a consumer
iii.observe({ function_id: 'cron::on-tick', args: { expression: '0 0 9 * * * *' } }, 'reports::daily')
```

A deliver hook can call targets itself, and what it returns is what the
function's caller gets back:

```javascript
iii.onObserverDeliver('http::on-request', async (call) => {
  const route = routes.match(call.input)
  if (!route) return { status_code: 404 }
  if (!route.obs.matches(call)) return { status_code: 422 }
  return iii.call({ observer: route.obs, payload: call.input })
})
```

## Resources

- [Documentation](https://iii.dev/docs)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)
