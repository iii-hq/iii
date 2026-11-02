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
| Observer hooks           | `onObserverAdded`, `onObserverRemoved`, `onObserverMatch`, `onObserverDeliver` | Change what observing a function this worker hosts means       |
| Call through an observer | `await iii.call({ observer, payload })`                                       | Run an observer's target from a deliver hook                    |
| Current invocation       | `iii.currentInvocation()`                                                     | The invocation the running handler serves (caller, activation)  |

`iii.trigger`, `TriggerAction` and `TriggerRequest` are deprecated aliases of
`iii.call`, `CallAction` and `CallRequest`: the same function, object and type.

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

Filters are evaluated where the call is. The engine evaluates them for every
call it routes, holding the call's input, caller and metadata and the
answer's output or error. The SDK evaluates them for the calls a worker makes
to its own functions in-process, which the engine never sees, and matches
each call through an index of the filters' `$eq` and `$in` conditions: with
1,000 observers that each filter on their own `input.tenant`, a call
evaluates one filter.

### Hosting Observable Functions

The worker that hosts a function decides what observing it means with four
hooks, each overriding a default:

| Hook                                 | Default                                                                       |
| ------------------------------------ | ----------------------------------------------------------------------------- |
| `onObserverAdded(obs)`               | accept the observer                                                           |
| `onObserverRemoved(obs)`             | stop delivering to it and forget it                                           |
| `onObserverMatch(obs, call)`         | every observer wants every call                                               |
| `onObserverDeliver(call, observers)` | run every matching observer whose filters match the call; return the handler's value |

`onObserverAdded` and `onObserverRemoved` can be set for every function the
worker hosts, or for one function by id or on its `FunctionRef`.
`onObserverMatch` and `onObserverDeliver` are set per function. Each method
returns its receiver, and `null` removes a hook.

An event source returns the event it is called with. `args_format` describes
the `args` its observers pass, and `private: true` lets only this worker call
it:

```javascript
import { ObserveRejection } from 'iii-sdk'

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
  .onObserverMatch((obs, call) => obs.args.expression === call.input.expression)

// a consumer
iii.observe({ function_id: 'cron::on-tick', args: { expression: '0 0 9 * * * *' } }, 'reports::daily')
```

With many observers, match by keys instead: the SDK keeps an index, one
lookup per key whatever the number of observers.

```javascript
iii.onObserverMatch('database::on-row-changed', {
  observer: (obs) => `${obs.args.table}|${obs.args.op}`, // what this observer listens for
  call: (call) => `${call.input.table}|${call.input.op}`, // what this call is
})
```

A deliver hook delivers each call itself: it receives the call and the
observers that matched it, and what it returns is what the function's caller
gets back:

```javascript
iii.onObserverDeliver('http::on-request', async (call) => {
  const route = routes.match(call.input)
  if (!route) return { status_code: 404 }
  if (!route.obs.matches(call)) return { status_code: 422 }
  return iii.call({ observer: route.obs, payload: call.input })
})
```

To send one observer something of its own, such as its initial state, call
the function with `metadata: { for_observer: obs.id }` once `obs.accepted()`
resolves, and let the match hook give that call to that observer alone. iii
delivers it like every call, so the target receives it in the shape it
receives every call of the function:

```javascript
iii.onObserverAdded('tmux::on-changed', (obs) => {
  void obs
    .accepted()
    .then(() =>
      iii.call({
        function_id: 'tmux::on-changed',
        payload: stateFor(obs.args),
        metadata: { for_observer: obs.id },
        action: CallAction.Void(),
      }),
    )
    .catch(() => {
      // a rejected observer gets nothing
    })
})
iii.onObserverMatch('tmux::on-changed', (obs, call) =>
  call.metadata?.for_observer ? call.metadata.for_observer === obs.id : obs.args.session === call.input.session,
)
```

With keys, file each observer under `for_observer:<id>` as well, and give such
a call that one key.

A worker's own call to its private function passes the input and the result
by reference: the handler and the hooks treat both as read-only, and with
`NODE_ENV=development` the SDK freezes them, so a write throws.

A function that sets `args_format`, `private: true`, `observable: false`, an
`onObserverMatch` hook or an `onObserverDeliver` hook needs an engine that
advertises `observe.v2`; against an older engine it stays unregistered, and the
SDK logs `UNSUPPORTED` once for it.

## Resources

- [Documentation](https://iii.dev/docs)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)
