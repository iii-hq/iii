# iii-browser-sdk

Browser SDK for the [iii engine](https://github.com/iii-hq/iii) — WebSocket-based, no Node.js dependencies, no OpenTelemetry.

[![npm](https://img.shields.io/npm/v/iii-browser-sdk)](https://www.npmjs.com/package/iii-browser-sdk)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](../../../LICENSE)

## Why the browser SDK

The browser SDK turns your frontend into an iii Worker enabling:

- **Persistent connection** — one WebSocket replaces many HTTP round-trips.
- **Bi-directional communication** — the engine can invoke functions registered in the browser. Backend workers push data to the frontend with `call()` or by observing it, enabling real-time patterns without polling.
- **Same API** — `registerFunction`, `call`, `observe` and the observer hooks — all the primitives you use server-side work identically in the browser.
- **Zero Node.js dependencies** — runs in any browser environment with native `WebSocket`.

## Install

```bash
pnpm add iii-browser-sdk
```

## Configure a public worker-manager listener

`iii-worker-manager` is engine-owned. Add an RBAC-protected `#instance` listener under
`engine.workers`, for example `iii-worker-manager#browser`; do not expose the private listener to
untrusted browser clients.

Refer to the [iii-worker-manager](https://workers.iii.dev/workers/iii-worker-manager) documentation for more information.

## Hello World

```typescript
import { registerWorker } from 'iii-browser-sdk'

const iii = registerWorker('ws://remotehost:3111')

iii.registerFunction('ui::show-notification', async (data: { title: string; body: string }) => {
  showToast(data.title, data.body)
  return { displayed: true }
})

const users = await iii.call({
  function_id: 'api::get::users',
  payload: {},
})
```

## API

| Operation                | Signature                                                                      | Description                                                    |
| ------------------------ | ------------------------------------------------------------------------------ | -------------------------------------------------------------- |
| Initialize               | `registerWorker(url, options?)`                                                | Connect to the engine via browser WebSocket. Returns `ISdk`    |
| Register function        | `iii.registerFunction(id, handler, options?)`                                  | Register a function that can be called by id and observed      |
| Call (await)             | `await iii.call({ function_id, payload })`                                     | Call a function and wait for the result                        |
| Call (fire-and-forget)   | `iii.call({ function_id, payload, action: CallAction.Void() })`                | Call without waiting                                           |
| Call (enqueue)           | `iii.call({ function_id, payload, action: CallAction.Enqueue({ queue }) })`    | Route the call through a named queue                           |
| Observe                  | `iii.observe('fn::id' \| { function_id, args?, filters? }, target, options?)`  | Call `target` on every matching call of a function             |
| Observer hooks           | `onObserverAdded`, `onObserverRemoved`, `onObserverMatch`, `onObserverDeliver` | Change what observing a function this page hosts means         |
| Call through an observer | `await iii.call({ observer, payload })`                                        | Run an observer's target from a deliver hook                   |
| Current invocation       | `iii.currentInvocation()` / `iii.withInvocation(inv, fn)`                      | The invocation the running handler serves (caller, activation) |
| Create channel           | `createChannel(iii)` (from `iii-browser-sdk/helpers`)                          | Create a streaming channel pair (writer + reader)              |
| Shutdown                 | `iii.shutdown()`                                                               | Gracefully disconnect from the engine                          |

`iii.trigger`, `TriggerAction` and `TriggerRequest` are deprecated aliases of
`iii.call`, `CallAction` and `CallRequest`: the same function, object and type.

### Registering Functions

Register a function in the browser that backend workers can call:

```typescript
iii.registerFunction('ui::show-notification', async (data: { title: string; body: string }) => {
  showToast(data.title, data.body)
  return { displayed: true }
})
```

The handler's return value is what the function's observers observe.

### Calling Backend Functions

Call any function registered in the engine directly from the browser:

```typescript
const users = await iii.call({
  function_id: 'api::get::users',
  payload: {},
})
```

A call can carry `metadata` (any JSON value, handed to the handler as its
second argument and readable by observer filters as `metadata.*`; `null` is the
same as none, and a call through an observer takes none), and a W3C
`traceparent` / `baggage` to continue a trace. The browser SDK has no
OpenTelemetry: it never makes up a trace context. Inside a handler, calls
forward the `traceparent` the handler was invoked with; a top-level call sends
the one you pass, or none.

```typescript
await iii.call({
  function_id: 'orders::create',
  payload: cart,
  metadata: { page: 'checkout' },
  traceparent: document.querySelector('meta[name="traceparent"]')?.getAttribute('content') ?? undefined,
})
```

A call to a function this worker registered itself runs in-process once the
engine confirmed it (no frames on the wire unless someone observes it). Pass
`dispatch: 'remote'` to go through the engine anyway, or register the function
with `localDispatch: false`.

### Observing Functions

Every function is observable. When `orders::ship` runs, its observers run. The
target is a registered function; it receives the `Observed` envelope (`input`,
`output` or `error`, `caller`, ...).

```typescript
import type { Observed } from 'iii-browser-sdk'

iii.registerFunction('ui::refresh-orders', async () => {
  await ordersTable.reload()
  return null
})

iii.registerFunction('ui::order-shipped', async ({ input }: Observed<{ order_id: string }>) => {
  showToast(`Order ${input?.order_id} shipped`)
  return null
})

// every successful call
iii.observe('orders::ship', 'ui::refresh-orders')

// only this customer's orders
const shipped = iii.observe(
  { function_id: 'orders::ship', filters: { 'input.customer_id': { $eq: customerId } }, capture: ['input'] },
  'ui::order-shipped',
)
await shipped.ready()
// later
await shipped.dispose()
```

The engine's own events are functions too: observe `EngineSources.FUNCTIONS_CHANGED`
(`engine::on-functions-changed`), `EngineSources.WORKERS_CHANGED` and the others
the same way.

### Hosting Observable Functions

The worker that hosts a function decides what observing it means with four
hooks, each overriding a default:

| Hook                                 | Default                                                                               |
| ------------------------------------ | ------------------------------------------------------------------------------------- |
| `onObserverAdded(obs)`               | accept the observer                                                                   |
| `onObserverRemoved(obs)`             | stop delivering to it and forget it                                                   |
| `onObserverMatch(obs, call)`         | every observer wants every call                                                       |
| `onObserverDeliver(call, observers)` | run every matching observer whose filters match the call; return the handler's value |

`onObserverAdded` and `onObserverRemoved` can be set for every function the
page hosts, or for one function by id or on its `FunctionRef`.
`onObserverMatch` and `onObserverDeliver` are set per function. Each method
returns its receiver, and `null` removes a hook.

An event source returns the event it is called with. `args_format` describes
the `args` its observers pass, and `private: true` lets only this page call it
(its input and result then pass by reference, never copied):

```typescript
import { ObserveRejection } from 'iii-browser-sdk'

iii
  .registerFunction('ui::on-click', (click: { selector: string; x: number; y: number }) => click, {
    args_format: { type: 'object', required: ['selector'], properties: { selector: { type: 'string' } } },
    private: true,
  })
  .onObserverAdded((obs) => {
    const selector = obs.args.selector as string
    if (!document.querySelector(selector)) throw new ObserveRejection('NO_SUCH_ELEMENT', `nothing matches ${selector}`)
    clicks.retain(selector, (event) =>
      iii.call({ function_id: 'ui::on-click', payload: { selector, x: event.clientX, y: event.clientY } }),
    )
  })
  .onObserverRemoved((obs) => clicks.release(obs.args.selector as string))
  // each click reaches the observers of its selector
  .onObserverMatch((obs, call) => obs.args.selector === call.input.selector)

// a backend worker observes it
// iii.observe({ function_id: 'ui::on-click', args: { selector: '#buy' } }, 'analytics::track-click')
```

With many observers, match by keys instead: the SDK keeps an index, one
lookup per key whatever the number of observers.

```typescript
iii.onObserverMatch('ui::on-click', {
  observer: (obs) => obs.args.selector as string, // what this observer listens for
  call: (call) => call.input.selector, // what this call is
})
```

`iii.isObserved('ui::on-click', { input })` (or `isObserved({ input })` on the
`FunctionRef`) says whether a call would reach anyone, so a source can skip
building an event nobody receives.

A deliver hook delivers each call itself: it receives the call and the
observers that matched it, and what it returns is what the function's caller
gets back. It applies the observers' filters with `obs.matches(call)` and
runs their targets with a call through the observer:

```typescript
iii.onObserverDeliver('ui::on-command', async (call) => {
  const handler = commands.match(call.input)
  if (!handler) return { handled: false }
  if (!handler.obs.matches(call)) return { handled: false }
  return iii.call({ observer: handler.obs, payload: call.input })
})
```

`obs.accepted()` resolves once the SDK acked the observer; a call made then
reaches it. A source sends a new observer its initial state as a call of the
function with `metadata: { for_observer: obs.id }`, which the match hook gives
to that observer alone, so the target receives it in the shape of every call:

```typescript
import { CallAction } from 'iii-browser-sdk'

iii.onObserverAdded('ui::on-click', (obs) => {
  void obs
    .accepted()
    .then(() =>
      iii.call({
        function_id: 'ui::on-click',
        payload: lastClick(obs.args.selector as string),
        metadata: { for_observer: obs.id },
        action: CallAction.Void(),
      }),
    )
    .catch(() => {
      // a rejected observer gets nothing
    })
})
iii.onObserverMatch('ui::on-click', (obs, call) =>
  call.metadata?.for_observer ? call.metadata.for_observer === obs.id : obs.args.selector === call.input.selector,
)
```

With keys, file each observer under `for_observer:<id>` as well, and give such
a call that one key.

With `standby: true`, a second page that registers a function another
connection already hosts waits and takes it over (with its observers) when that
one leaves.

A function that sets `args_format`, `private: true`, `observable: false`, an
`onObserverMatch` hook or an `onObserverDeliver` hook needs an engine that
advertises `observe.v2`; against an older engine it stays unregistered, and the
SDK logs `UNSUPPORTED` once for it.

### The current invocation across `await`

`iii.currentInvocation()` returns the invocation the running handler serves
(its id, caller, activation, `observe_depth`, `traceparent`). Calls made from a
handler carry it, so the engine can tell who called whom and stop observe
loops.

Browsers have no `AsyncLocalStorage`, so the SDK knows the invocation only
while the handler runs synchronously, **up to its first `await`**; after it,
`currentInvocation()` returns `undefined`. Read the invocation first and
re-enter it with `withInvocation` for what comes after:

```typescript
iii.registerFunction('ui::checkout', async (cart) => {
  const inv = iii.currentInvocation() // before the first await
  const quote = await fetchQuote(cart)
  return iii.withInvocation(inv, () => iii.call({ function_id: 'orders::create', payload: quote }))
})
```

`withInvocation(inv, fn)` runs `fn` in that invocation and returns what `fn`
returns; the calls made inside it, through observers included, belong to the
invocation too. When `fn` is async, the invocation counts as running until its
promise settles.

A deliver hook runs in its function's invocation the same way: in scope up to
its first `await`. A call through an observer is routed by the observer alone,
so it reaches the observer's target from anywhere, after an `await` included.

Calls made after an `await` without `withInvocation` are not treated as new
chains. They carry the `observe_depth` of the deepest invocation running on the
page, so the engine's loop guard (`max_observe_depth`, 8 by default) still
stops an observer cycle whose target awaits before it calls back. The cost of
guessing: with several invocations running at once the depth is over-counted,
and a legitimate chain may be stopped early. Such calls carry a `caller` and
`traceparent` only when that is unambiguous (exactly one invocation is running,
and it is an observe activation or its function's deliver hook is running);
otherwise they carry neither. An invocation that never settles stops counting
as running after 10 minutes (a local call: at its timeout). Local calls to this
worker's own functions are nested the same way: a recursion that awaits before
each call still goes through the engine past 64 levels, while calls started
together (a loop, `Promise.all`) count as one level. A known limitation: local
calls started in separate ticks while earlier ones are still running look
nested too, so a long-running, wide fan-out may send some of its calls through
the engine (same results, more frames). While the socket is closed that never
happens, since the engine could not take the call anyway: a function this
worker hosts then runs here up to 256 local calls deep, and past that the call
fails at once with the code `LOCAL_DEPTH_EXCEEDED`, so a runaway recursion
neither overflows the stack nor freezes the page. Use `withInvocation` to say
exactly which invocation a call belongs to.

### Receiving Live Invocations

Backend workers can push data to the browser in real time. No polling required:

```typescript
iii.registerFunction('ui::update-dashboard', async (metrics: { cpu: number; memory: number; requests: number }) => {
  document.getElementById('cpu')!.textContent = `${metrics.cpu}%`
  document.getElementById('memory')!.textContent = `${metrics.memory}MB`
  document.getElementById('requests')!.textContent = `${metrics.requests}/s`
  return null
})
```

## Resources

- [Documentation](https://iii.dev/docs)
- [Use iii in the Browser](https://iii.dev/docs/how-to/use-iii-in-the-browser)
- [Browser SDK API Reference](https://iii.dev/docs/api-reference/sdk-browser)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)
