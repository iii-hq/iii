---
title: observe v2 — functions, observers, and defaults you override
tagline: "Every function is observable and there is one delivery path: a function runs, its observers fire. The worker that hosts a function changes what that means with four hooks: onObserverAdded and onObserverRemoved, for one function or the whole worker, and onObserverMatch and onObserverDeliver, for one function."
date: 2026-10-29
tags: [engine, sdk, protocol, observe, dx]
status: draft
---

# observe v2 — functions, observers, and defaults you override

> Design. Status: **draft**. Defines how a worker hosts observable functions.
> It builds on [`2026-10-13-observe`](../2026-10-13-observe/README.md)
> (shipped in 0.25) and keeps its consumer API, filter language
> ([`filters.md`](../2026-10-13-observe/filters.md)), observer lifecycle,
> policy, loop guard and local dispatch. Where this document says nothing,
> the 0.25 design applies. Triggers are removed (§11), and `call` is how a
> function is invoked (§3.1). Implementation decisions are in
> [`decisions.md`](decisions.md).

## 1. The model

There are two things: **functions** and **observers**.

```ts
iii.observe('orders::create', 'alerts::big-order')
```

When `orders::create` runs, `alerts::big-order` runs. That is the consumer
model for every function: a plain function, a cron tick, a row change, an HTTP
request. What an observer observes is the function's return value.

The worker that hosts a function decides what observing it means with four
hooks. Each one overrides something iii does by default:

| Hook | The question it answers | What iii does by default |
|---|---|---|
| `onObserverAdded(obs)` | What do I set up for this observer? Install a database trigger, start a cron job, add an HTTP route, open a queue consumer. Throw to reject it. | accepts the observer: it becomes active and receives the function's calls |
| `onObserverRemoved(obs)` | What do I tear down? | stops delivering to the observer and forgets it |
| `onObserverMatch(obs, call)` | Does this observer want this call? A cron observer of `0 9 * * *` wants that expression's ticks. | every observer wants every call |
| `onObserverDeliver(call, observers)` | I deliver this call myself: which observers, and what does the caller get back? Route an HTTP request to one route and return its response; ack a queue message with its subscriber's result. | runs the target of every matching observer whose filters match the call, and gives the caller the handler's return value |

**Scope.** `onObserverAdded` and `onObserverRemoved` can be set for one
function and for every function the worker hosts, and both levels run: when an
observer is added, the worker's hook and then the function's; when it is
removed, the function's and then the worker's. `onObserverMatch` and
`onObserverDeliver` are set per function. A function can set both: its
`onObserverDeliver` receives the observers that matched.

```ts
// every function this worker hosts: keep the event stream open while anything is observed
iii.onObserverAdded(async () => { await events.retain() })
iii.onObserverRemoved(async () => { await events.release() })

// one function, by id
iii.onObserverAdded('database::on-row-changed', async (obs) => {
  await changeFeed.watch(obs.args.db, obs.args.table)
})

// one function, chained off its registration
iii.registerFunction('database::on-row-changed', (change: RowChange) => change, { private: true, args_format: RowChangedArgs })
  .onObserverRemoved(async (obs) => { await changeFeed.unwatch(obs.args.db, obs.args.table) })
```

**Args and filters.** `args` belong to the worker that hosts the function.
iii validates them against the function's `args_format` and hands them to its
hooks; the worker decides what they mean (a table to capture, a cron
expression, a route, a retry policy). `filters` belong to iii. They select
calls by `input`, `output`, `error`, `caller` and `metadata`, and iii evaluates
them wherever the call is delivered: in the worker that ran it, or in the
engine.

## 2. From 0.25

| 0.25 | v2 |
|---|---|
| `registerFunction(id, handler, { observe: { delivery, args_format, onObserverAdded, onObserverRemoved, allow, legacy_trigger_type } })` | `registerFunction(id, handler, { args_format?, private?, observable?, standby? })`; hooks are methods on the client or on the returned `FunctionRef` (§4.2) |
| `delivery: 'auto' \| 'owner'` | one delivery path; a function can set `onObserverMatch` and `onObserverDeliver` |
| `args_format` with `delivery: 'owner'` | `args_format` on any function |
| `obs.emit(event)` | call the function; its `onObserverMatch` says which observers want it |
| `obs.call(payload)` | `iii.call({ observer: obs, payload })` |
| an owner-delivered function refuses calls from other workers | `private: true` (§6) |
| `observe.allow: false` | `observable: false` |
| `registerTrigger({ type, function_id, config, metadata })` | `iii.observe({ function_id, args, filters }, target, { metadata })` |
| `registerTriggerType(…)` + `TriggerHandler` | a function with `args_format` and hooks |
| `iii.trigger(request)` | `iii.call(request)`; `trigger` is an alias |
| CLI `iii trigger <fn>` | `iii call <fn>`; `iii trigger` is an alias |
| `Observe::owner()`, `Observe::auto()`, `ObserveHandler` (Rust) | `on_observer_added`, `on_observer_removed`, `on_observer_match`, `on_observer_deliver` methods taking closures or types that implement the hook traits (§5.2) |
| `observe({ function_id }, target)` | also `observe('fn::id', target)` |
| consumer option `capture` | removed; `deliver` picks what the target receives |
| the engine evaluates routed calls, the host evaluates local calls and emits | the host evaluates every call of its functions; the engine evaluates calls to hosts without `observe.v2` and to engine-hosted sources (§8.2) |
| error codes `NOT_OWNER_DELIVERED`, `FILTERS_NOT_MATCHED`, `TARGET_MISMATCH`, `INVALID_OBSERVE_DESCRIPTOR` | retired |

## 3. Consumer API

```ts
// every successful call of orders::create runs alerts::big-order
iii.observe('orders::create', 'alerts::big-order')

// full form
iii.observe(
  {
    function_id: 'database::on-row-changed',
    args: { db: 'primary', table: 'orders', ops: ['insert', 'update'] },  // for the database worker
    filters: { 'caller.parent.worker_name': { $ne: 'order-projector' } },   // for iii
    deliver: 'output',
  },
  'orders::project',
  { max_in_flight: 1 },
)
```

- **`args`** are read by the function's worker. The function's description
  says what they mean; the database worker reads `{ db, table, ops }` to
  capture that table and to deliver only its changes.
- **`filters`** select which calls reach the target, by anything in the call.
- **`deliver`** is what the target receives when iii delivers: the `Observed`
  envelope (default: ids, status, timing, `input`, `output` or `error`,
  `caller` and `metadata`), the call's `input`, or its `output`. A function whose
  `onObserverDeliver` calls targets itself documents what they receive
  (`http::on-request` targets receive the `HttpRequest`).
- The handle (`ready()`, `state`, `onStateChange`, `dispose()`) and the
  options (`id`, `metadata`, `max_fires`, `expires_in_ms`, `max_in_flight`)
  are the 0.25 ones.

### 3.1 Calling functions

`call` invokes a function and resolves to its result:

```ts
const order = await iii.call({ function_id: 'orders::create', payload: { sku: 'A1', qty: 2 } })
await iii.call({ function_id: 'audit::log', payload: entry, action: CallAction.Void() })   // fire-and-forget
```

| SDK | Call | Alias |
|---|---|---|
| TypeScript, browser | `iii.call(request)`, `CallRequest`, `CallAction` | `iii.trigger`, `TriggerRequest`, `TriggerAction` |
| Rust | `iii.call(CallRequest::new(id, payload))`, `CallAction` | `iii.trigger`, `TriggerRequest`, `TriggerAction` |
| Python | `iii.call(request)`, `iii.call_async(request)` | `trigger`, `trigger_async` |
| Go | `client.Call(ctx, iii.CallRequest{…})`, `iii.CallAction` | `client.Trigger`, `iii.TriggerRequest`, `iii.TriggerAction` |
| CLI | `iii call <function_id>`, with the arguments `iii trigger` takes | `iii trigger` |

Each alias is the same function or type under its 0.25 name, so code written
against 0.25 keeps compiling. Examples, docs and every worker use `call`.

## 4. Provider API (TypeScript)

### 4.1 Registering

```ts
iii.registerFunction<I, O>(id: string, handler: (input: I) => O | Promise<O>, options?: FunctionOptions): FunctionRef<I, O>
```

**The handler's return value is what observers observe.** It is the `output`
in their envelope and in their filters. An event source's handler returns the
event:

```ts
iii.registerFunction('cron::on-tick', (tick: CronTick) => tick, options)
```

These options describe the function to the engine:

| Option | Default | Meaning |
|---|---|---|
| `description`, `metadata` | — | as in 0.25 |
| `request_format` | — | schema of the call's input |
| `response_format` | — | schema of the handler's return value: what observers observe |
| `args_format` | none | JSON Schema for observers' `args`; the engine validates them (`INVALID_ARGS`). A function without it takes no args. |
| `private` | `false` | only this worker may call the function; others may observe it (§6) |
| `observable` | `true` | `false`: nobody may observe it (`NOT_OBSERVABLE`); for functions whose payloads carry credentials |

This one is handled by the SDK:

| Option | Default | Meaning |
|---|---|---|
| `standby` | `false` | when another connection already hosts the id, wait and take over when it leaves (§7) |

What the worker does when its function is observed is set with hooks (§4.2).

### 4.2 Hooks

```ts
type AddedHook   = (obs: Observer) => void | Promise<void>
type RemovedHook = (obs: Observer) => void | Promise<void>
type MatchHook<I, O> =
  | ((obs: Observer, call: Call<I, O>) => boolean)                  // a predicate
  | { observer: (obs: Observer) => Key, call: (call: Call<I, O>) => Key }   // keys, indexed (§4.5)
type Key = string | readonly string[]
type DeliverHook<I, O, R = O> =
  (call: Call<I, O>, observers: readonly Observer[]) => R | Promise<R>   // returns what the caller gets back

interface III {
  onObserverAdded(hook: AddedHook | null): III                            // every function this worker hosts
  onObserverAdded(functionId: string, hook: AddedHook | null): III        // one function
  onObserverRemoved(hook: RemovedHook | null): III
  onObserverRemoved(functionId: string, hook: RemovedHook | null): III
  onObserverMatch<I, O>(functionId: string, hook: MatchHook<I, O> | null): III
  onObserverDeliver<I, O, R>(functionId: string, hook: DeliverHook<I, O, R> | null): III

  observers(functionId: string): readonly Observer[]
  isObserved(functionId: string, call?: Partial<Call>): boolean
  registerFunction(…): FunctionRef
  // call (§3.1), observe, … 
}

// the same methods with the function id filled in
interface FunctionRef<I = unknown, O = unknown> {
  readonly id: string
  onObserverAdded(hook: AddedHook | null): FunctionRef<I, O>
  onObserverRemoved(hook: RemovedHook | null): FunctionRef<I, O>
  onObserverMatch(hook: MatchHook<I, O> | null): FunctionRef<I, O>
  onObserverDeliver<R = O>(hook: DeliverHook<I, O, R> | null): FunctionRef<I, O>
  observers(): readonly Observer[]
  isObserved(call?: Partial<Call<I>>): boolean
  unregister(): void
}
```

These three do the same thing:

```ts
iii.registerFunction('cron::on-tick', (tick: CronTick) => tick, opts).onObserverAdded(start)

const onTick = iii.registerFunction('cron::on-tick', (tick: CronTick) => tick, opts)
onTick.onObserverAdded(start)

iii.onObserverAdded('cron::on-tick', start)
```

- **By id.** The id form needs no `FunctionRef`, so hooks can be grouped apart
  from the registrations or set before the function is registered. Hooks are
  stored by function id and stay in place across `unregister()` and a later
  registration of the same id. Once the worker's connection is registered, the
  SDK logs a warning for each hook id the worker has not registered, which
  catches a mistyped id.
- **Chaining.** Each method returns its receiver. `null` removes the hook.
- **Order.** When an observer is added, the worker's `onObserverAdded` runs,
  then the function's. When it is removed, the function's `onObserverRemoved`
  runs, then the worker's.

### 4.3 Observer lifecycle in the worker

- **Accepted.** An observer is accepted when every `onObserverAdded` that
  applies to it has resolved. The SDK then acks it, and it becomes active.
- **Rejected.** If an `onObserverAdded` throws, the observer is rejected with
  the thrown `ObserveRejection` (`code`, `message`), or `OBSERVE_REJECTED` for
  any other error. The `onObserverRemoved` of each level whose `onObserverAdded`
  already resolved runs, so a worker hook that retained a resource releases it.
- **Removed.** `onObserverRemoved` runs exactly once for every accepted
  observer: on `unobserve`, on disposal, and on late rejection with
  `obs.reject()`. It never runs for an observer that was not accepted. An error
  it throws is logged; the observer is removed either way.
- **One at a time.** For each function, the SDK runs observer hooks one at a
  time, in the order the engine announced the observers. Hooks of different
  functions run concurrently.
- **Timeout.** A hook that runs past 30 s fails the observer with
  `OWNER_TIMEOUT`, as in 0.25.

**When hooks take effect.** The SDK acks no observer of a function before the
call that registered it has returned. So a hook set by id before
`registerFunction`, or chained on its result, runs for every observer. In Node
and the browser this also covers a hook set later in the same synchronous
block, since inbound messages run on the event loop. In Rust and Go set hooks
by id before registering (§5.2).

A hook set once a function already has active observers is **caught up**: the
SDK runs it for each of them at once, a throw late-rejects that observer, and
the SDK logs one warning naming the function and the number of observers that
were active without it.

### 4.4 Calls and delivery

```ts
interface Call<I = unknown, O = unknown> {
  function_id: string
  invocation_id: string
  status: 'success' | 'error'
  input: I
  output?: O                                  // the handler's return value: what observers observe
  error?: { code: string; message: string }   // what the handler threw
  caller?: Caller
  metadata?: Record<string, unknown>
}
```

Every call of a function, local or routed, runs in the worker that hosts it,
in three steps:

1. **The handler runs.** Its return value is what observers observe.
2. **Observers are matched.** By default every observer of the function wants
   every call. `onObserverMatch` narrows that (§4.5).
3. **The call is delivered.** By default iii delivers it (below).
   `onObserverDeliver` takes delivery over (§4.6).

iii's delivery:

1. selects, among the matching observers, those whose `on` matches
   `call.status` and whose filters match the call;
2. hands them to the engine, which runs each target with the observer's
   `deliver` choice, policy, metadata sidecar and loop guard;
3. gives the caller the handler's return value, or rethrows its error.

It does not wait for targets. A call with no observers costs one atomic load.
The caller and the observers see the same value.

### 4.5 Matching observers

`onObserverMatch` answers one question: does this observer want this call? A
worker sets it when its args say which calls an observer wants. iii does
everything else as usual: the observer's filters, `on`, policy and `deliver`
still apply.

As a predicate:

```ts
// an observer of expression X gets the ticks of expression X
iii.onObserverMatch('cron::on-tick', (obs, call) => obs.args.expression === call.input.expression)
```

As keys, when a function has many observers. An observer wants a call when
they share a key, and iii keeps an index instead of asking every observer:

```ts
// an observer of orders/insert gets inserts into orders
iii.onObserverMatch('database::on-row-changed', {
  observer: (obs) => `${obs.args.table}|${obs.args.op}`,     // what this observer listens for
  call: (call) => `${call.input.table}|${call.input.op}`,     // what this call is
})
```

- Each key function returns a string or an array of strings.
- The SDK computes an observer's keys once, when it is accepted, and files the
  observer under each; it unfiles it when it is removed.
- A call matches the observers filed under any of its keys, each once.
- Keys express wildcards by listing them. The database files an observer of
  every table under `"primary|*|insert"`, and a change matches
  `"primary|orders|insert"`, `"primary|orders|*"`, `"primary|*|insert"` and
  `"primary|*|*"`: four lookups, whatever the number of observers.

**`isObserved(call?)`** answers whether a call would reach anyone, so a source
can skip building an event nobody wants. Without a call it is true when the
function has any active observer: one atomic load. With a call, on a function
matched by keys, it is true when an observer is filed under one of the call's
keys: one hash lookup per key. On any other function the call is ignored.

### 4.6 Delivering yourself

`onObserverDeliver` is for a function whose observers' answers matter, or
whose calls each go to one observer. The hook receives the call and the
observers that matched it (a snapshot taken when the call started), delivers
the call itself, and returns what the caller gets back. iii delivers nothing
for that function.

```ts
// http::on-request: each request goes to one route; its target's result is the response
iii.onObserverDeliver('http::on-request', async (call) => {
  const route = routes.match(call.input)
  if (!route) return { status_code: 404 }
  if (!route.obs.matches(call)) return { status_code: 422 }
  return iii.call({ observer: route.obs, payload: route.request })
})
```

The HTTP server gets the route's response, while observers of
`http::on-request` observe the request.

**Calling through an observer.** A call that names an observer runs that
observer's target:

```ts
iii.call({ observer: obs, payload })                                   // resolves to the target's result
iii.call({ observer: obs, payload, action: CallAction.Void() })        // fire-and-forget
```

- Only the worker that hosts the observer's function may call through it
  (`NOT_FUNCTION_OWNER`).
- The engine routes the call to the observer's stored target and applies its
  policy (`max_fires`, expiry, `max_in_flight`), metadata sidecar and loop
  guard, as for iii's delivery.
- The call sends the payload as given. The hook applies the observer's filters
  with `obs.matches(call)`, as above.

### 4.7 `Observer`

```ts
interface Observer {
  readonly id: string
  readonly function_id: string
  readonly args: Record<string, unknown>      // validated against args_format
  readonly filters: Filter                    // normalized
  readonly target: string                     // function id
  readonly on: 'success' | 'error' | 'complete'
  matches(call: Partial<Call>): boolean       // `on` and filters
  accepted(): Promise<void>                   // resolves once the SDK has acked the observer
  reject(code: string, message: string): void // late rejection
}

class ObserveRejection extends Error {        // thrown by onObserverAdded to reject with a code
  constructor(readonly code: string, message: string)
}
```

`accepted()` lets a provider send an observer its initial state: a call
through the observer made after it resolves reaches an active observer, since
the engine handles a connection's frames in order.

An observer is a record of what was asked for. The worker sees the fields
above; who observes, the observer's metadata and its policy stay with the
engine.

## 5. Every SDK

### 5.1 TypeScript (node, browser)

As in §4. Hooks may be sync or async.

### 5.2 Rust

The same methods as TypeScript, on the client and on `FunctionRef`. Rust has
no overloading, so the client's per-function form takes a `_for` suffix:

| TypeScript | Rust |
|---|---|
| `iii.onObserverAdded(hook)` | `iii.on_observer_added(hook)` |
| `iii.onObserverAdded(id, hook)` | `iii.on_observer_added_for(id, hook)` |
| `iii.onObserverRemoved(hook)` | `iii.on_observer_removed(hook)` |
| `iii.onObserverRemoved(id, hook)` | `iii.on_observer_removed_for(id, hook)` |
| `iii.onObserverMatch(id, hook)` | `iii.on_observer_match_for(id, hook)` |
| `iii.onObserverDeliver(id, hook)` | `iii.on_observer_deliver_for(id, hook)` |
| `fn.onObserverAdded(hook)`, `fn.onObserverRemoved(hook)`, `fn.onObserverMatch(hook)`, `fn.onObserverDeliver(hook)` | `function.on_observer_added(hook)`, `function.on_observer_removed(hook)`, `function.on_observer_match(hook)`, `function.on_observer_deliver(hook)` |

A hook is any value that implements the hook's trait. `Observer` is a cheap,
cloneable handle passed by value, so async closures implement the traits, and
a worker with shared state implements them on its own type.

```rust
#[async_trait]
pub trait AddedHook: Send + Sync + 'static {
    async fn on_added(&self, obs: Observer) -> Result<(), ObserveRejection>;
}
#[async_trait]
pub trait RemovedHook: Send + Sync + 'static {
    async fn on_removed(&self, obs: Observer);
}
#[async_trait]
pub trait DeliverHook: Send + Sync + 'static {
    async fn on_deliver(&self, call: Call, observers: Vec<Observer>) -> Result<Value, Error>;
}

// onObserverMatch: a predicate, or keys that iii indexes
pub enum Match {
    Predicate(Arc<dyn Fn(&Observer, &Call) -> bool + Send + Sync>),
    Keys { observer: Arc<dyn Fn(&Observer) -> Vec<String> + Send + Sync>,
           call: Arc<dyn Fn(&Call) -> Vec<String> + Send + Sync> },
}
impl Match {
    pub fn predicate(f: impl Fn(&Observer, &Call) -> bool + Send + Sync + 'static) -> Self;
    pub fn keys(observer: impl Fn(&Observer) -> Vec<String> + …, call: impl Fn(&Call) -> Vec<String> + …) -> Self;
}

// one function, by id: set before registering, so it sees every observer (§4.3)
iii.on_observer_added_for("database::on-row-changed", change_feed.clone());   // ChangeFeed: AddedHook
iii.on_observer_removed_for("database::on-row-changed", change_feed.clone()); // ChangeFeed: RemovedHook
iii.on_observer_match_for("database::on-row-changed", Match::keys(observer_keys, call_keys));

iii.register_function(
    "database::on-row-changed",
    RegisterFunction::new(|change: RowChangedEvent| Ok::<_, Error>(change))
        .args_format::<RowChangedArgs>()
        .private(),
);

// every function this worker hosts, as a closure
let events = events.clone();
iii.on_observer_added(move |_obs: Observer| {
    let events = events.clone();
    async move { events.retain().await; Ok(()) }
});

// through an observer
iii.call(CallRequest::observer(&obs, payload)).await?;
```

### 5.3 Python

```python
row_changed = iii.register_function("database::on-row-changed", lambda change: change,
                                    {"args_format": RowChangedArgs, "private": True})

@row_changed.on_observer_added
async def install(obs):
    await change_feed.watch(obs.args["db"], obs.args.get("table"))

@iii.on_observer_added("cron::on-tick")         # by function id
async def start(obs): ...

@iii.on_observer_added                          # every function this worker hosts
async def retain(obs): ...

row_changed.on_observer_match(observer=observer_keys, call=call_keys)      # keys
# or: row_changed.on_observer_match(lambda obs, call: obs.args["table"] == call.input["table"])

await iii.call_async({"observer": obs, "payload": payload})
```

The methods work as decorators and as plain calls.

### 5.4 Go

```go
rowChanged := client.RegisterFunction("database::on-row-changed", iii.Identity,   // returns the event it is called with
    iii.FunctionOptions{ArgsFormat: rowChangedArgs, Private: true})

rowChanged.OnObserverAdded(func(ctx context.Context, obs *iii.Observer) error {
    return changeFeed.Watch(ctx, obs.Args)
})
client.OnObserverAddedFor("cron::on-tick", start)   // by function id
client.OnObserverAdded(retain)                      // every function this worker hosts
rowChanged.OnObserverMatch(iii.MatchKeys(observerKeys, callKeys))   // or iii.MatchFunc(func(obs *iii.Observer, call *iii.Call) bool { … })

out, err := client.Call(ctx, iii.CallRequest{Observer: obs, Data: payload})
```

## 6. `private` functions

An event source is observed by everyone and called only by its own worker. A
call from another worker would be a forged tick, request or row change
carrying the source's identity.

`private: true` expresses that:

- The engine accepts an `InvokeFunction` of a private function only from the
  connection that hosts it. Any other connection, and the engine itself
  (hooks, middleware), gets `FORBIDDEN`.
- A private function is observable, and its observers receive every call its
  worker makes.
- A private function cannot be an observer's target (`FORBIDDEN` at `observe`).
- **A worker's call to its own private function passes the input and the
  result by reference.** Nobody else can see either value, so the SDK copies
  neither, and the handler and hooks treat both as read-only. Development
  builds of the Node SDK freeze them, so a write throws. An observer's target
  receives its own copy, serialized when the call matches.

Any function can be private; event sources are the common case.

## 7. Hosts and instances

A function id has one host per namespace: the engine rejects a second
registration of the same id with `FUNCTION_NAMESPACE_CONFLICT`. Observer hooks
therefore run once per observer, in that host, and a cron expression or a
database trigger is set up once however many instances of the worker run.

A second instance serves as a standby. When the host disconnects, the
function's observers go `pending`; the standby registers the function,
receives the current observers in `functionregistered`, runs its hooks for
each, and they become active again. With `standby: true`, the SDK does this:
on `FUNCTION_NAMESPACE_CONFLICT` it waits on `engine::on-functions-changed` and
registers when the id is free.

## 8. Wire protocol

### 8.1 Feature

SDKs and the engine advertise `observe.v2`. §8 applies between two
`observe.v2` parties; §11 covers the rest.

### 8.2 Who delivers

The worker that hosts a function delivers its calls:

- **Local call.** Matches go to the engine in a batched `invocationsobserved`
  report, as in 0.25. Nothing is sent when nothing matches.
- **Routed call.** The engine always sends an `invocation_id` to the host, so
  every routed call has an `invocationresult`. The SDK runs the handler and
  delivers, and lists the observers iii's delivery selected on that result.
  The result is the handler's return value, which the engine already holds
  with the input, so a match costs no extra frame. A function with
  `onObserverDeliver` lists none: its hook calls through observers itself.

```jsonc
{ "type": "invocationresult", "invocation_id": "c3f…", "result": { … },
  "observed": ["5b0c…", "a07…"] }          // omitted when empty
```

  The engine activates each listed observer after checking that it belongs
  to the invoked function and is active. The envelope carries the call's
  input and, as `output`, the handler's return value.
- **Through an observer.** An `invokefunction` with `activation.observer_id`.
  The engine checks that the observer exists, that the sender hosts its
  function, and that it is active, then applies policy and routes to the
  observer's stored target.

**A handler that returns its input.** When the handler's return value is its
input (the same value, as for every event source), an `invocationsobserved`
item carries the value once and sets `"output_is_input": true`; the engine
fills `output` from `input`. Event sources send each event once, whatever the
observers' `deliver` asks for. A routed call needs no flag: the engine takes `output` from
the `invocationresult` it already receives.

The engine evaluates filters itself for calls to hosts without `observe.v2`
and for engine-hosted sources.

### 8.3 `registerfunction`

```jsonc
{ "type": "registerfunction", "id": "database::on-row-changed",
  "request_format": { … }, "response_format": { … }, "args_format": { … },
  "private": true }
```

`private: false` and `observable: true` are the defaults and are omitted.
Hooks and `standby` stay in the SDK. The 0.25 `observe` object is decoded from
any connection (decisions 1, 37).

### 8.4 Observer messages

`observeradded`, `observerremoved`, `observerack` and `functionregistered` are
the 0.25 messages; §4.3 says when the SDK sends `observerack`.

### 8.5 Error codes

| Code | When |
|---|---|
| `NOT_FUNCTION_OWNER` | engine: a call through an observer from a connection that does not host the observer's function |
| `OBSERVER_NOT_FOUND`, `OBSERVER_NOT_ACTIVE`, `OVERFLOW`, `LOOP_GUARD`, `TIMEOUT` | engine: activation checks and policy, as in 0.25 §3.5 |
| `FORBIDDEN` | engine: a call to a private function from another connection; a private function as a target; RBAC |
| `OBSERVE_REJECTED`, `OWNER_TIMEOUT` | an `onObserverAdded` threw a plain error, or ran past 30 s |
| `UNSUPPORTED` | SDK: a function that needs `observe.v2` on an engine without it (§11) |

All other codes are the 0.25 ones.

## 9. Engine changes

| Change | Where |
|---|---|
| Advertise `observe.v2`; record it per connection | `engine/src/engine/mod.rs` |
| For routed calls to `observe.v2` hosts, skip evaluation and activate the ids in `invocationresult.observed` | `engine/src/invocation/mod.rs`, `engine/src/observer/` |
| `output_is_input` on `invocationsobserved` items | `engine/src/protocol.rs`, `engine/src/observer/` |
| Top-level `args_format`, `private`, `observable` on `RegisterFunction`; `args_format` on any function | `engine/src/protocol.rs`, `engine/src/observer/registry.rs` |
| `private`: accept invocations only from the host; refuse it as a target | invoke path in `engine/src/engine/mod.rs`; `observe` validation |
| Activations: check observer, host and state, then policy | `engine/src/observer/` |
| `invocationsobserved`: accept call reports only | `engine/src/observer/` |
| Remove the trigger registry, trigger messages, trigger builtins, the trigger RBAC hook, the legacy shim and owner delivery; warn once per connection and message type on a removed trigger message | `engine/src/trigger.rs`, `engine/src/protocol.rs`, `engine/src/engine/mod.rs`, `engine/src/observer/` |
| `iii call` subcommand, with `iii trigger` as its alias | `engine/src/main.rs`, `engine/src/cli_trigger/` |

Registry, lifecycle, revisions, acks, policy, loop guard, observer builtins,
metrics and the observer RBAC hook keep their 0.25 behavior.

## 10. The workers

Checked against `iii-hq/workers` at `43f0799`. The workers are Rust; the
examples are TypeScript for brevity, and §5.2 is the Rust shape.

### 10.1 database: `database::on-row-changed`

`onObserverAdded` installs native capture, plain code calls the function, and
`onObserverMatch` gives each change to the observers of its database, table and
operation, by keys.

```ts
const rowKey = (db: string, table: string, op: string) => `${db}|${table}|${op}`

const rowChanged = iii.registerFunction('database::on-row-changed', (change: RowChange) => change, {
  request_format: RowChangeSchema, response_format: RowChangeSchema,
  args_format: RowChangedArgsSchema, private: true,
})
  .onObserverAdded(async (obs) => {
    const { db, table } = obs.args as RowChangedArgs
    if (!config.has(db)) throw new ObserveRejection('UNKNOWN_DB', `no database '${db}'`)
    if (config.native(db)) await changeFeed.watch(db, table)   // CREATE TRIGGER … pg_notify(…)
  })
  .onObserverRemoved(async (obs) => {
    await changeFeed.unwatch(obs.args.db, obs.args.table)        // drops the trigger with its last observer
  })
  .onObserverMatch({
    observer: ({ args }) => (args.ops ?? ['*']).map((op) => rowKey(args.db, args.table ?? '*', op)),
    call: ({ input }) => [input.table, '*'].flatMap((t) => [input.op, '*'].map((op) => rowKey(input.db, t, op))),
  })

// a notification from the database becomes a call
changeFeed.onChange(async (n) => {
  const change = toRowChange(n)
  if (!rowChanged.isObserved({ input: change })) return
  await iii.call({ function_id: 'database::on-row-changed', payload: change })
})
```

On a `capture: statements` database, `database::execute` calls the function
after each committed write, so `caller.parent` names the worker that wrote the
row. The args are `{ db, table?, ops? }`. In
`workers/database/src/triggers/capture.rs`, observers live in the match
index and the handler returns the change it is called with.

### 10.2 cron: `cron::on-tick`

`onObserverAdded` starts the expression's job; `onObserverMatch` gives each
tick to that expression's observers.

```ts
iii.registerFunction('cron::on-tick', (tick: CronTick) => tick, {
  request_format: CronTickSchema, response_format: CronTickSchema,
  args_format: CronArgsSchema, private: true,
})
  .onObserverAdded((obs) => {
    const { expression } = obs.args as CronArgs
    scheduler.retain(expression, (scheduled_time) =>        // throws INVALID_CRON_EXPRESSION
      iii.call({ function_id: 'cron::on-tick',
        payload: { expression, scheduled_time, actual_time: now() } }))
  })
  .onObserverRemoved((obs) => scheduler.release(obs.args.expression))
  .onObserverMatch((obs, call) => obs.args.expression === call.input.expression)
```

`scheduler.retain` runs one job per distinct expression, as `scheduler.rs`
does, and `release` stops it with its last observer.

### 10.3 http: `http::on-request`

`onObserverDeliver` routes each request to one observer, and that target's
result is the response.

```ts
const routes = new RouteTable()

iii.registerFunction('http::on-request', (request: HttpRequest) => request, {
  request_format: HttpRequestSchema, response_format: HttpRequestSchema,
  args_format: HttpRouteArgsSchema, private: true,
})
  .onObserverAdded((obs) => routes.add(obs))                 // throws HTTP_ROUTE_CONFLICT
  .onObserverRemoved((obs) => routes.remove(obs.id))
  .onObserverDeliver<HttpResponse>(async (call) => {
    const match = routes.match(call.input.method, call.input.path)   // /users/:id → params
    if (!match) return { status_code: 404 }
    if (!match.obs.matches(call)) return { status_code: 422 }
    await runMiddleware(match, call.input)
    return iii.call({ observer: match.obs, payload: { ...call.input, params: match.params } })
  })

server.on('request', async (req, res) =>
  send(res, await iii.call({ function_id: 'http::on-request', payload: toHttpRequest(req) })))
```

The request and the response pass by reference between the server and the
hook (§6). Every request is a call of `http::on-request` in traces.

### 10.4 queue: `queue::on-message`

Each observer is a subscriber with its own consumer, retries and group. A
message is a call of the function with the subscriber's observer id in its
metadata, and the target's result acks it.

```ts
iii.registerFunction('queue::on-message', (message: QueueMessage) => message, {
  request_format: QueueMessageSchema, response_format: QueueMessageSchema,
  args_format: SubscriberSpecSchema, private: true,
})
  .onObserverAdded((obs) => consumers.start(obs.id, obs.args))     // INVALID_ARGS, OBSERVER_ID_CONFLICT
  .onObserverRemoved((obs) => consumers.stop(obs.id))
  .onObserverMatch({ observer: (obs) => obs.id, call: (call) => call.metadata?.subscriber ?? '' })
  .onObserverDeliver<unknown>(async (call, [obs]) => {
    if (!obs) throw new ObserveRejection('OBSERVER_NOT_ACTIVE', 'subscriber removed')   // retried, then dead-lettered
    if (!obs.matches(call)) return null                      // a filter miss acks
    return iii.call({ observer: obs, payload: call.input })
  })

consumers.onMessage((subscriber, message) =>
  iii.call({ function_id: 'queue::on-message', payload: message, metadata: { subscriber } }))
```

Consumers' filters read the message as `input`. Each subscriber calls
through its observer, and `call_outcome` maps the target's result to ack,
retry or dead-letter.

### 10.5 state: `state::on-changed`

No hooks: a function observed with filters on `input.scope` and `input.key`,
with `isObserved()` as its fast path.

### 10.6 Every other event

Every other trigger type in `iii-hq/workers` (browser, github, voice,
worktree, session-manager, computer, tmux and the rest) becomes a function
whose handler returns the event, called when the event happens, and the
worker's subscriber set goes away. A worker that should do work only while something is
watching uses the worker-wide hooks:

```ts
iii.onObserverAdded(() => cdp.retain())
iii.onObserverRemoved(() => cdp.release())
```

## 11. Triggers removed, compatibility and migration

Triggers are removed from the engine, every SDK and every worker: trigger
types, `registerTrigger`, `registerTriggerType`, `unregisterTrigger`,
`unregisterTriggerType`, `TriggerHandler`, the engine's trigger registry, the
trigger wire messages (`registertriggertype`, `unregistertriggertype`,
`registertrigger`, `unregistertrigger`, `triggerregistrationresult`), the
engine builtins that register and list triggers, the trigger RBAC hook, and the
0.25 owner-delivery API (`observe.delivery`, `obs.emit`, `obs.call`). A trigger
type is an observable function; a binding is an observer.

The engine answers each removed trigger message with one `warn` log per
connection and message type, naming the replacement, and drops the frame.

| | 0.25 engine | v2 engine |
|---|---|---|
| **0.25 SDK** | as shipped | functions, `trigger` and `observe` work. A function registered with `observe.delivery: owner` is refused with `UNSUPPORTED`; trigger messages are dropped as above. |
| **v2 SDK** | functions that need `observe.v2` (`args_format`, `private`, `observable: false`, or `onObserverDeliver`) stay unregistered, and the SDK logs `UNSUPPORTED` once per function; it registers them when the worker connects to an engine with `observe.v2`. Every other function, `call` and `observe` work, and the engine evaluates routed calls. | full |

Migrating:

| 0.25 | v2 |
|---|---|
| `registerTrigger({ type, function_id, config, metadata })` | `iii.observe({ function_id: <the type's function>, args: config }, function_id, { metadata })` |
| `registerTriggerType(…)` + `TriggerHandler` | a function with `args_format`, `private: true` and hooks (§10) |
| `observe: { delivery: 'owner', args_format, onObserverAdded, onObserverRemoved }` | `{ args_format, private: true }`, then `.onObserverAdded(…)` / `.onObserverRemoved(…)` |
| an index from args to observers, and `obs.emit(event)` | call the function with the event; `.onObserverMatch(…)` (§10.1, §10.2) |
| `obs.call(payload)` | `.onObserverDeliver(…)` with `iii.call({ observer: obs, payload })` (§10.3, §10.4) |
| a handler that refuses calls | a handler that returns the event, and `private: true` |
| `observe: { allow: false }` | `{ observable: false }` |
| `iii.trigger(request)` | `iii.call(request)` |
| `iii trigger <fn>` | `iii call <fn>` |

Trigger types and the functions that replace them:

| Trigger type | Function | Args |
|---|---|---|
| `http` | `http::on-request` | `{ method, path, middleware_function_ids?, public_webhook? }` |
| `cron` | `cron::on-tick` | `{ expression }` |
| `database::row-changed` | `database::on-row-changed` | `{ db, table?, ops? }` |
| `durable:subscriber` | `queue::on-message` | `{ topic, max_retries?, backoff_ms?, queue_config?, group? }` |
| `subscribe` | `pubsub::publish`, observed with a filter on `input.topic` | none |
| `state` | `state::on-changed` | none; filters on `input.scope`, `input.key` |
| `log`, `trace` | `observability::on-log`, `observability::on-span` | none |
| `configuration` | `configuration::on-updated` | none; the event carries `{ id, event_type }`, and `configuration::get` returns the value |
| `engine::functions-available`, `engine::workers-available` | `engine::on-functions-changed`, `engine::on-workers-changed` | none |
| `stream`, `stream:join`, `stream:leave` | `stream::on-changed`, `stream::on-join`, `stream::on-leave` | none; filters on `input.stream_name`, `input.group_id` |
| — | `engine::on-observers-changed` (new) | none |

Every other trigger type `T` of a worker `W` becomes `W::on-<event>`, listed
in that worker's README.

## 12. Guarantees

The 0.25 guarantees (§6 there) hold, with these:

- **Completeness.** An observer becomes `active` after its `onObserverAdded`
  hooks have resolved, so its database trigger or cron job exists before the
  first call it should see. A hook set late is caught up and logged (§4.3).
- **Setup and teardown pair up.** Every resolved `onObserverAdded` is matched
  by exactly one `onObserverRemoved` at the same level (§4.3).
- **Delivery.** iii's delivery is at-most-once, with drops counted per
  function and reason. A function that calls targets itself delivers as its
  `onObserverDeliver` does: one call per HTTP request; retries and dead letters
  per queue subscriber.
- **Filters.** iii's delivery applies the observers' `on` and filters. An
  `onObserverDeliver` hook applies them with `obs.matches(call)`.
- **Hooks once.** A function has one host per namespace, so each observer's
  hooks run once per host registration (§7).

## 13. Performance budget

Measured in phase 2 on a Rust host at 5k calls/s, and set for Node after the
phase 2 baseline.

| Scenario | Budget |
|---|---|
| Call, no observers | one atomic load, no allocation, 0 frames |
| Function matched by keys, observers on other keys (`isObserved(call)` false) | one hash lookup per call key, no allocation |
| Matching by keys, 1 matching observer among 10,000 | ≤ 4 hash lookups + one filter evaluation + 1 report item |
| iii's delivery, 10 observers, none matching | < 5 µs (compiled filters) |
| Routed call, observed | 0 extra frames (ids on `invocationresult`) |
| Event source (handler returns its input), observed | the event serialized once |
| Local call to a private function | no copy of input or result |
| Call through an observer | one routed invocation |

## 14. Execution plan

1. **Engine.** `observe.v2` (§9), `private`, routed delivery by the host,
   `output_is_input`, and the removal of triggers and owner delivery (§11).
   *Done when* the engine suite passes with the host delivering routed and
   local calls, 1M unobserved calls send zero frames, an event source sends
   each event once, a private function refuses calls from another connection,
   a standby takes over a function's observers when its host leaves, and no
   trigger code remains.
2. **SDKs: Rust, Node, browser, Python, Go.** The three hooks at both levels,
   `onObserverMatch` (predicate and keys), `onObserverDeliver`,
   `isObserved(call)`, calls through an observer, `call` with
   `trigger` as its alias, `private`, `standby`, the string shorthand, the
   unregistered-id warning, refusal on engines without `observe.v2`, and no
   trigger API. *Done when* each SDK's suite passes, including tests for hook
   order at both levels, rejection unwinding, one removal per accepted
   observer, hooks one at a time per function, catch-up and its warning, and
   key indexing under add and remove.
3. **CLI.** `iii call`, with `iii trigger` as its alias.
4. **Workers.** database, cron, http and queue (§10), then every other trigger
   type and every binding in `iii-hq/workers`. *Done when* each worker's suite
   passes against the v2 engine and SDKs and no trigger code remains.
5. **Docs.** The consumer page opens with `iii.observe('a', 'b')` and `call`;
   the provider page is the hooks table in §1 and the four workers; READMEs,
   skills and agent docs use `call` and observers.
6. **Verification.** Unit, integration and cross-SDK suites; engine benches;
   load tests against §13.

## 15. Decisions

1. **Observer hooks run at both levels.** The worker's and the function's
   `onObserverAdded` both run, worker first; removal runs in reverse. Setup
   and teardown pair up per level.
2. **Matching and delivering are separate hooks.** `onObserverMatch` answers
   "does this observer want this call?" and leaves delivery to iii.
   `onObserverDeliver` takes delivery over and decides what the caller gets
   back. Both are per function.
3. **Hooks are methods, descriptions are options.** What the engine needs
   (`args_format`, `private`, `observable`, formats) is an option of
   `registerFunction`. What the worker does is a hook on the client, by
   function id or for all its functions, or on the `FunctionRef`.
4. **Args belong to the worker, filters to iii.** Args set things up and
   choose observers in `onObserverMatch`. Filters select calls; iii's delivery and
   `obs.matches` apply them.
5. **The host delivers.** One evaluation per call, in the worker that ran it;
   routed matches ride the result frame.
6. **Observers observe the handler's return value,** and `response_format`
   describes it. `onObserverDeliver` decides what the caller gets back; iii's
   delivery gives it the same value.
7. **Calling through an observer is a call.** The engine checks the
   host and applies the observer's policy; the payload goes as given.
8. **One host per function.** Hooks run once per observer; extra instances
   are standbys.
9. **`private` is general,** and a worker's own calls to a private function
   pass values by reference.
10. **`call` is the verb.** Every SDK and the CLI invoke functions with
    `call`; `trigger` is its alias. Triggers as a concept are gone: events are
    functions, bindings are observers.

## 16. Open questions

- **Key functions in the engine.** Matching by keys runs in the host. If the engine
  should prune activations for hosts without `observe.v2`, the keys would
  need a declarative form. No current worker needs it.
- **Standby election.** Several standbys race to register when the host
  leaves; the engine accepts the first. Whether the engine should order them
  is left until a worker runs more than one standby.
