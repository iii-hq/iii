# iii-browser-sdk

Browser SDK for the [iii engine](https://github.com/iii-hq/iii) — WebSocket-based, no Node.js dependencies, no OpenTelemetry.

[![npm](https://img.shields.io/npm/v/iii-browser-sdk)](https://www.npmjs.com/package/iii-browser-sdk)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](../../../LICENSE)

## Why the browser SDK

The browser SDK turns your frontend into an iii Worker enabling:

- **Persistent connection** — one WebSocket replaces many HTTP round-trips.
- **Bi-directional communication** — the engine can invoke functions registered in the browser. Backend workers push data to the frontend with `trigger()`, enabling real-time patterns without polling.
- **Same API** — `registerFunction`, `trigger`, `observe` — all the primitives you use server-side work identically in the browser.
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

const users = await iii.trigger({
  function_id: 'api::get::users',
  payload: {},
})
```

## API

| Operation                | Signature                                                                         | Description                                                    |
| ------------------------ | --------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| Initialize               | `registerWorker(url, options?)`                                                   | Connect to the engine via browser WebSocket. Returns `ISdk`    |
| Register function        | `iii.registerFunction(id, handler, options?)`                                     | Register a function the engine (or backend) can invoke         |
| Invoke (await)           | `await iii.trigger({ function_id, payload })`                                     | Invoke a function and wait for the result                      |
| Invoke (fire-and-forget) | `iii.trigger({ function_id, payload, action: TriggerAction.Void() })`             | Invoke without waiting                                         |
| Invoke (enqueue)         | `iii.trigger({ function_id, payload, action: TriggerAction.Enqueue({ queue }) })` | Route invocation through a named queue                         |
| Observe                  | `iii.observe({ function_id, args?, filters? }, target, options?)`                 | Invoke `target` on every matching invocation of a function     |
| Current invocation       | `iii.currentInvocation()` / `iii.withInvocation(inv, fn)`                         | The invocation the running handler serves (caller, activation) |
| Create channel           | `createChannel(iii)` (from `iii-browser-sdk/helpers`)                             | Create a streaming channel pair (writer + reader)              |
| Shutdown                 | `iii.shutdown()`                                                                  | Gracefully disconnect from the engine                          |

### Registering Functions

Register a function in the browser that backend workers can call:

```typescript
iii.registerFunction('ui::show-notification', async (data: { title: string; body: string }) => {
  showToast(data.title, data.body)
  return { displayed: true }
})
```

### Calling Backend Functions

Invoke any function registered in the engine directly from the browser:

```typescript
const users = await iii.trigger({
  function_id: 'api::get::users',
  payload: {},
})
```

A call can carry `metadata` (any JSON, handed to the handler as its second
argument and readable by observer filters as `metadata.*`), and a W3C
`traceparent` / `baggage` to continue a trace. The browser SDK has no
OpenTelemetry: it never makes up a trace context. Inside a handler, calls
forward the `traceparent` the handler was invoked with; a top-level call sends
the one you pass, or none.

```typescript
await iii.trigger({
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

Every function is observable. The target is a registered function; it receives
the `Observed` envelope (`input`, `output` or `error`, `caller`, ...).

```typescript
iii.registerFunction('ui::order-shipped', async ({ input }: Observed<{ order_id: string }>) => {
  showToast(`Order ${input?.order_id} shipped`)
  return null
})

const shipped = iii.observe(
  { function_id: 'orders::ship', filters: { 'input.customer_id': { $eq: customerId } }, capture: ['input'] },
  'ui::order-shipped',
)
await shipped.ready()
// later
await shipped.dispose()
```

A function can also hand its own events to the observers they belong to (owner
delivery, `registerFunction(id, handler, { observe: { delivery: 'owner', onObserverAdded, onObserverRemoved } })`)
and check `FunctionRef.isObserved()` before building an event nobody receives.

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
  return iii.withInvocation(inv, () => iii.trigger({ function_id: 'orders::create', payload: quote }))
})
```

`withInvocation(inv, fn)` runs `fn` in that invocation and returns what `fn`
returns; `obs.emit` and `obs.call` made inside it belong to the invocation too.
When `fn` is async, the invocation counts as running until its promise settles.

Calls made after an `await` without `withInvocation` are not treated as new
chains. They carry the `observe_depth` of the deepest invocation running on the
page, so the engine's loop guard (`max_observe_depth`, 8 by default) still
stops an observer cycle whose target awaits before it calls back. The cost of
guessing: with several invocations running at once the depth is over-counted,
and a legitimate chain may be stopped early. Such calls carry a `caller` and
`traceparent` only when that is unambiguous (exactly one invocation is running,
and it is an observe activation); otherwise they carry neither. They never
borrow an invocation's metadata, so `obs.emit` / `obs.call` filters on
`metadata.*` only see it inside a real scope. An invocation that never settles
stops counting as running after 10 minutes (a local call: at its timeout).
Local calls to this worker's own functions are nested the same way: a recursion
that awaits before each call still goes through the engine past 64 levels,
while calls started together (a loop, `Promise.all`) count as one level. A
known limitation: local calls started in separate ticks while earlier ones are
still running look nested too, so a long-running, wide fan-out may send some of
its calls through the engine (same results, more frames). While the socket is
closed that never happens, since the engine could not take the call anyway: a
function this worker hosts then runs here up to 256 local calls deep, and past
that the call fails at once with the code `LOCAL_DEPTH_EXCEEDED`, so a runaway
recursion neither overflows the stack nor freezes the page. Use
`withInvocation` to say exactly which invocation a call belongs to.

### Trigger APIs (deprecated)

`registerTrigger` and `registerTriggerType` keep working in 0.25 but are
deprecated: they log a warning once per page. Use `iii.observe(source,
target)` and `registerFunction(id, handler, { observe })` instead. See the
[migration guide](https://iii.dev/docs/upgrading/from-0-24-x).

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
