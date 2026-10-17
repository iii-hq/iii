# SDK: API and internals

The file:line references are to `sdk/packages/*` at `05fafde11` (iii v0.24.5).
They are written `N:` for `node/iii/src/iii.ts`, `R:` for
`rust/iii/src/iii.rs`, `PY:` for `python/iii/src/iii/iii.py` and `GO:` for
`go/iii/client.go`.

These references are to the tree the plan was written against; the 0.25
implementation moved the code (`observe*.ts`, `observable.*`, `reporter.*`,
`invocation-context.*` in each SDK). Where this file and the code differ, the code
wins and the difference is a decision recorded in the README.

## 1. Provider API: observable functions

`registerTriggerType` is replaced (in 0.25 it keeps working, deprecated; phase 6
removes it). A function that wants to know who observes it passes an `observe`
descriptor to `registerFunction`.

### TypeScript (node, browser)

```ts
interface Observer {
  readonly id: string
  readonly function_id: string
  readonly args: Record<string, unknown>                 // the observer's args, validated against args_format by the engine
  readonly filters: Filter                               // the observer's filters, normalized; applied by the SDK, never edited by the owner
  readonly target: string                                // function id (never the observer's metadata or policy)
  readonly capture: Capture[]
  readonly on: 'success' | 'error' | 'complete'          // auto only
  readonly legacy?: { trigger_type: string; config: unknown }   // phase 5 shim
  matches(doc: { input?: unknown; output?: unknown; error?: unknown; caller?: Caller; metadata?: unknown }): boolean
  emit(event: unknown): boolean                          // owner only: fire-and-forget; false when the filters said no or the observer is no longer active
  call<R = unknown>(payload: unknown, opts?: { timeoutMs?: number; metadata?: never }): Promise<R>   // owner only: request/response
  reject(code: string, message: string): void            // late rejection of an active observer; returns nothing
}

class ObserveRejection extends Error {
  constructor(public code: string, message: string) { super(message) }
}

interface ObserveHooks {
  delivery?: 'auto' | 'owner'                            // default 'auto'; args_format requires 'owner'
  args_format?: JSONSchema                               // owner only; validates observe args in the engine (INVALID_ARGS)
  legacy_trigger_type?: string
  allow?: boolean                                        // default true
  onObserverAdded?(obs: Observer): void | Promise<void>  // throw → rejected; after 30 s → OWNER_TIMEOUT
  onObserverRemoved?(obs: Observer): void | Promise<void>
}

interface RegisterFunctionOptions {
  description?: string
  request_format?: JSONSchema
  response_format?: JSONSchema
  metadata?: Record<string, unknown>
  observe?: ObserveHooks                                  // new
  localDispatch?: boolean                                 // new; default true
}

interface FunctionRef {
  id: string
  unregister(): void
  isObserved(): boolean                                   // new: one load, for the emit fast path
  observers(): readonly Observer[]                        // new: active observers, as the owner sees them
}
```

An owner that emits (the whole cron provider surface). The engine has already
checked `args` against `args_format`, so a missing `expression` never reaches
`onObserverAdded`:

```ts
// cron worker: hosts cron::on-tick and decides which observers get each tick
const schedules = new Map<string, Set<Observer>>()         // expression → observers

const onTick = iii.registerFunction('cron::on-tick', (tick: CronTick) => {
  // one call per fire: hand the tick to the observers of that expression
  for (const obs of schedules.get(tick.expression) ?? []) obs.emit(tick)
}, {
  description: 'Fires on a cron schedule. Observe it with args { expression }.',
  request_format: cronTickSchema,              // { expression, scheduled_time }: what filters read as input.*
  observe: {
    delivery: 'owner',                         // this worker picks who receives each tick
    args_format: { type: 'object', additionalProperties: false, required: ['expression'],
                   properties: { expression: { type: 'string' } } },   // INVALID_ARGS otherwise
    onObserverAdded(obs) {                     // someone observed cron::on-tick
      const { expression } = obs.args
      if (!schedules.has(expression)) {        // first observer of this expression: start one job
        scheduler.start(expression, (scheduled_time) =>   // throws on a bad expression → ready() rejects
          iii.trigger({ function_id: 'cron::on-tick', payload: { expression, scheduled_time } }))
        schedules.set(expression, new Set())
      }
      schedules.get(expression)!.add(obs)
    },
    onObserverRemoved(obs) {                   // dispose(), or its worker left for good
      const observers = schedules.get(obs.args.expression)!
      observers.delete(obs)
      if (observers.size === 0) {              // last observer of this expression: stop its job
        scheduler.stop(obs.args.expression)
        schedules.delete(obs.args.expression)
      }
    },
  },
})
```

An owner that calls (HTTP routing, abridged from [`http.md`](http.md)):

```ts
const routes = new RouteTable()

iii.registerFunction('http::on-request', async (req: HttpRequest) => {
  const hit = routes.match(req.method, req.path)                 // built from the observers' args
  if (!hit) return notFound()
  const request = { ...req, route: hit.route, path_params: hit.params }
  if (!hit.obs.matches({ input: request })) return conditionNotMet()   // the observer's filters → 422
  return hit.obs.call<HttpResponse>(request)                     // the target's result is the response
}, {
  request_format: httpRequestSchema,
  response_format: httpResponseSchema,
  observe: {
    delivery: 'owner',
    args_format: httpRouteArgsSchema,     // { method, path, public_webhook?, middleware_function_ids?, condition_function_id? }
    legacy_trigger_type: 'http',
    onObserverAdded(obs) { routes.insert(obs.id, obs.args) },    // throws HTTP_ROUTE_CONFLICT
    onObserverRemoved(obs) { routes.remove(obs.id) },
  },
})
```

Rules:

- `onObserverAdded` runs once per added observer, before the SDK acks it. A throw
  (any error; `ObserveRejection` sets the code, otherwise `OBSERVE_REJECTED`)
  rejects the observer. It returns nothing.
- **Args in, events out.** `obs.args` tells the owner what to set up and which
  events belong to the observer. The owner keeps its own index (expression →
  observers, route table, `(db, table)` → observers) and hands each event
  to the observers it belongs to: `obs.emit(event)` when no result is
  needed, `obs.call(payload)` when it is. It never writes filters: the SDK
  applies the observer's filters on every `emit` and `call`.
- **Index before the ack.** Whatever `onObserverAdded` adds to the owner's index is
  in place when the ack is sent, so the first event after the engine marks the
  observer `active` reaches it.
- `obs.emit(event)`:
  - returns `false` and sends nothing when the observer is no longer active
    (removed or rejected meanwhile), so an owner's fan-out loop needs no guard
    and never aborts because one observer went away concurrently;
  - builds `{ input: event, caller, metadata }`; `caller` is what a call made at
    that point would carry: inside the observed function's own handler the
    event **is** that invocation (its id, its caller); inside another handler,
    that handler is the caller; outside any handler, `{ worker_name }` only;
  - returns `false` and sends nothing when the observer's filters reject it;
  - otherwise pushes a report item (§4.4), carrying the current invocation's
    `traceparent` / `baggage` so the target's activation joins the observed
    trace, returns `true`, and the engine
    activates the target with the `Observed` envelope (per `capture` /
    `deliver`), fire-and-forget, at most once.
- `obs.call(payload)` checks the filters too: a miss rejects with
  `FILTERS_NOT_MATCHED` and sends nothing. On a removed or rejected observer it
  rejects with `OBSERVER_NOT_ACTIVE`. Otherwise it is an `InvokeFunction`
  to the target with `activation` (and no `namespace`), and resolves with the
  target's result.
- `obs.emit()` and `obs.call()` throw `NOT_OWNER_DELIVERED` on an auto function.
- Callbacks for one function run one at a time, in revision order. Callbacks
  of different functions may run concurrently.
- A callback still running after 30 s is treated as a rejection with
  `OWNER_TIMEOUT` (the engine enforces the same timeout) and the next callback
  starts, so a hung callback cannot stall its function forever. Callbacks of
  one function may therefore overlap after an `OWNER_TIMEOUT` (document it on the
  hooks type); if the timed-out `onObserverAdded` completes later,
  `onObserverRemoved` runs for that observer.
- `onObserverRemoved` cannot fail; errors are logged.
- `obs.reject(code, message)` sends a late rejection for an active observer
  (e.g. after a config reload) and returns nothing, in every language. The
  observer leaves the compiled set at once; `onObserverRemoved` runs when the
  engine's `observerremoved` arrives. The rejection is stored on the observer,
  so it survives a reconnect: if the socket was down, or a later
  `functionregistered` lists the observer again, the SDK acks again with that
  error, using the revision the engine last announced for the observer (an
  older one is ignored as stale), and does not reactivate it until a fresh
  `observeradded`. A rejection made while `onObserverAdded` is still running
  wins: after the hook returns, the SDK acks with it instead of activating.
- `args_format` with `delivery: 'auto'` throws at `registerFunction`
  (`INVALID_OBSERVE_DESCRIPTOR`): an auto function's events are its
  invocations, offered to every observer, so there is nothing to route by.

### Rust

`TriggerHandler` becomes `ObserveHandler`, with the same shape:

```rust
#[async_trait]
pub trait ObserveHandler: Send + Sync + 'static {
    async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection>;
    async fn on_observer_removed(&self, obs: &Observer);
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CronArgs { expression: String }

#[async_trait]
impl ObserveHandler for Schedules {
    async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
        let CronArgs { expression } = obs.args()?;                      // already validated by the engine
        self.scheduler.retain(&expression, obs.clone()).await           // one job per distinct expression
            .map_err(|e| ObserveRejection::new("INVALID_CRON_EXPRESSION", e.to_string()))
    }
    async fn on_observer_removed(&self, obs: &Observer) { self.scheduler.release(obs.id()).await }
}

let on_tick = iii.register_function(
    "cron::on-tick",
    RegisterFunction::new({ let s = schedules.clone(); move |tick: CronTick| {
            for obs in s.observers(&tick.expression) { obs.emit(&tick)?; }      // filters checked here; Ok(false) on a removed observer
            Ok::<_, Error>(())
        }})
        .description("Fires on a cron schedule. Observe it with args { expression }.")
        // request_format is taken from the handler's input type (CronTick: JsonSchema);
        // .request_format(json!({ … })) overrides it with a Value
        .observe(Observe::owner().args_format::<CronArgs>().handler(schedules.clone())),
);

// in the scheduler task, once per fire of an expression: runs the handler in-process
iii.trigger(TriggerRequest::new("cron::on-tick", json!(tick))).await?;

// an owner that calls
iii.register_function(
    "http::on-request",
    RegisterFunction::new_async(move |req: HttpRequest| router.clone().route(req)),
    // …
    .observe(Observe::owner()
        .args_format::<HttpRouteArgs>()
        .legacy_trigger_type("http")
        .handler(HttpObserveHandler { routes })),
);
// inside the router: let res: HttpResponse = obs.call(&request).await?;
```

`Observer::args<T: DeserializeOwned>() -> Result<T, ObserveRejection>`
(`args_value() -> &Value` for the raw JSON), `filters() -> &Value`,
`matches(&Doc) -> bool`, `emit<T: Serialize + ?Sized>(&T) -> Result<bool, Error>`
(`Ok(false)` when the filters say no or the observer is inactive),
`call<R: DeserializeOwned>(payload: impl Serialize) -> Result<R, Error>`,
`reject(code, message)` (returns `()`). `id()`, `function_id()`,
`namespace()`, `target()`, `capture()`, `on()`, `legacy() -> Option<&LegacyTrigger>`
and `is_active()` are methods, not fields. `Observer` is a cheap clone (an
`Arc`), so owners keep it in their own index. `Observe::args_format::<T>()`
builds the schema from `T: JsonSchema`; `args_format_value(Value)` takes a
schema as is. `RegisterFunction::local_dispatch(false)` opts out of local
dispatch.

### Python

```python
class CronHooks(ObserveHooks):
    delivery = "owner"
    args_format = {"type": "object", "additionalProperties": False, "required": ["expression"],
                   "properties": {"expression": {"type": "string"}}}

    async def on_observer_added(self, obs: Observer) -> None:
        scheduler.retain(obs.args["expression"], obs)   # raises ObserveRejection("INVALID_CRON_EXPRESSION", …)

    async def on_observer_removed(self, obs: Observer) -> None:
        scheduler.release(obs.id)

def on_tick(tick):
    for obs in scheduler.observers(tick["expression"]):
        obs.emit(tick)                                   # filters checked here

iii.register_function("cron::on-tick", on_tick, observe=CronHooks())
# once per fire: await iii.trigger_async({"function_id": "cron::on-tick", "payload": tick})
# handlers and hooks may be sync or async; obs.emit() is sync, obs.call() / obs.call_async() return the target's result
```

### Go

```go
iii.RegisterFunctionTyped(client, "cron::on-tick", func(ctx context.Context, tick CronTick) (any, error) {
    for _, obs := range scheduler.Observers(tick.Expression) {
        obs.Emit(ctx, tick)                                       // (bool, error); false when filters say no or the observer is inactive
    }
    return nil, nil
}, iii.FunctionOptions{                                           // = iii.RegisterFunctionOptions
    Observe: &iii.ObserveHooks{
        Delivery:   iii.DeliveryOwner,
        ArgsFormat: cronArgsSchema,
        OnObserverAdded: func(ctx context.Context, obs *iii.Observer) error {
            expr, _ := obs.Args["expression"].(string)          // required by ArgsFormat
            if err := scheduler.Retain(expr, obs); err != nil {
                return iii.ObserveRejection("INVALID_CRON_EXPRESSION", err.Error())
            }
            return nil
        },
        OnObserverRemoved: func(ctx context.Context, obs *iii.Observer) { scheduler.Release(obs.ID) },
    },
})
// once per fire: _, err = client.Trigger(ctx, iii.TriggerRequest{FunctionID: "cron::on-tick", Data: tickJSON})
// obs.Args is a map (numbers are float64); obs.DecodeArgs(&v) fills a struct. obs.Reject(code, msg) returns nothing.
// LocalDispatch: &false on the options opts a function out of local dispatch.
```

## 2. Observer API

`iii.observe(source, target, options?)`. The first argument is the left side,
what is observed: the function, its `args` and the `filters`. The second is the
right side: the function to invoke. The third, optional, is the observer's
policy and metadata. It mirrors iii lang's `source(args) | predicate ~> target`
(the filter stage there is planned; README §12).

### TypeScript (node, browser)

```ts
type Capture = 'input' | 'output' | 'error' | 'caller' | 'metadata'
type ObserverState = 'pending' | 'active' | 'suspended' | 'error' | 'disposed'
type Filter = Record<string, unknown>                    // filters.md; explicit operators only
type Target = string | { function_id: string; namespace?: string }   // always a registered function

interface Caller {
  worker_id?: string        // stamped by the engine; absent when the SDK built the caller before it knew its id, or for an engine caller
  worker_name?: string
  function_id?: string      // the handler the call was made from
  invocation_id?: string
  parent?: Omit<Caller, 'parent'>
}

interface Observed<I = unknown, O = unknown> {
  observer_id: string
  activation_id: string
  function_id: string
  namespace: string
  invocation_id: string
  status: 'success' | 'error'
  input?: I
  output?: O
  error?: { code: string; message: string }
  caller?: Caller
  metadata?: Record<string, unknown>
  started_at_ms: number
  duration_ms: number
}

interface ObserveSource {                 // left side: what is observed
  function_id: string
  namespace?: string
  args?: Record<string, unknown>          // owner-delivered functions only: for the owner; validated against observe.args_format
  filters?: Filter                        // which events reach the target; evaluated by the engine / host SDK
  on?: 'success' | 'error' | 'complete'   // auto only; default 'success'
  capture?: Capture[]                     // auto and obs.emit; default ['input', 'output', 'caller']
  deliver?: 'envelope' | 'input' | 'output'   // auto and obs.emit; default 'envelope'
}

interface ObserverOptions {               // the arrow itself
  id?: string                             // default: random UUID, stable across replays
  max_fires?: number
  expires_in_ms?: number
  max_in_flight?: number
  metadata?: Record<string, unknown>      // the activation's metadata sidecar
}

interface ObserverHandle {
  readonly id: string
  readonly state: ObserverState
  readonly reason?: { code: string; message: string }
  ready(): Promise<void>                // resolves on 'active'; rejects with an ObserveRejection (carrying the code) on 'error' / 'disposed'
  onStateChange(listener: (state: ObserverState, reason?: { code: string; message: string }) => void): () => void
  dispose(): Promise<void>
}

iii.observe(source: ObserveSource, target: Target, options?: ObserverOptions): ObserverHandle
```

- `onStateChange` listeners never run on the connection's read loop (a listener
  that calls the client would deadlock it): they are delivered asynchronously,
  in order, per handle.
- `observe()` returns a handle synchronously, like `registerTrigger` does
  today. It works before the connection is up and is replayed on reconnect with
  the same id.
- Errors (`INVALID_ARGS`, `INVALID_FILTER`, `FORBIDDEN`, an owner rejection
  such as `HTTP_ROUTE_CONFLICT`) show up as state `error` and reject `ready()`.
- **There is no inline handler.** The target is registered first with
  `registerFunction`, as the iii lang compiler does for a lambda on the right
  of `~>`. Until the target is registered the observer stays `pending`.
- The target's first argument depends on how the event was delivered:
  - **auto**: the `Observed` envelope (or `input` / `output` with `deliver`);
    its return value is discarded;
  - **owner, `obs.emit`** (cron, database): the `Observed` envelope with
    `input`, `caller` and `metadata` (or `input` with `deliver: 'input'`); its
    return value is discarded;
  - **owner, `obs.call`** (HTTP, queue): the owner's payload (an
    `HttpRequest`, a queue message); its return value goes back to the owner
    (the HTTP response).

Examples:

```ts
// owner, emit: a cron schedule; args for the cron worker, filters checked on each tick
iii.registerFunction('local::cron-handler', ({ input }: Observed<CronTick>) => sendDigest(input))

const digest = iii.observe({
  function_id: 'cron::on-tick',
  args: { expression: '0 0 9 * * Mon-Fri' },
  filters: { 'input.scheduled_time': { $gte: '2026-11-01T00:00:00Z' } },   // start in November
}, 'local::cron-handler')
await digest.ready()

// owner, emit: changes to orders, except my own writes
iii.observe({
  function_id: 'database::on-row-changed',
  args: { db: 'primary', table: 'orders', ops: ['insert', 'update'] },
  filters: { 'caller.parent.worker_name': { $ne: 'order-projector' } },   // ignore my own writes
  capture: ['input', 'caller'],
}, 'orders::project')

// owner, call: an HTTP route; the target's return value is the response
iii.observe({
  function_id: 'http::on-request',
  args: { method: 'GET', path: '/users/:id' },
  filters: { 'input.headers.x-tenant': { $eq: 'acme' } },
}, 'users::get')

// auto, on a plain function: every function is observable (no args)
iii.observe({
  function_id: 'payments::charge',
  on: 'error',
  filters: { 'input.amount': { $gte: 500 } },
  capture: ['input', 'error', 'caller'],
}, 'alerts::payment-failed', { max_in_flight: 4 })
```

The current invocation is available in every handler, both remote and local:

```ts
const inv = iii.currentInvocation()   // { function_id, invocation_id, caller, activation?, observe_depth } | undefined
```

In the browser SDK there is no `AsyncLocalStorage`: the context is a
synchronous scope, set while a handler runs until its first `await`, and
`iii.withInvocation(inv, fn)` re-enters a captured one. After an `await`, an
outgoing call carries the `observe_depth` of the deepest running invocation and
a `caller` only when that is unambiguous (exactly one invocation is running
and it is part of an observe chain). The browser `trigger()` also sends
`metadata`, `traceparent` and `baggage`, and handlers receive the metadata as
their second argument.

### Rust

```rust
let digest = iii.observe(
    Source::function("cron::on-tick")
        .args(json!({ "expression": "0 0 9 * * Mon-Fri" }))
        .filters(json!({ "input.scheduled_time": { "$gte": "2026-11-01T00:00:00Z" } })),
    "local::cron-handler",
);
digest.ready().await?;

let orders = iii.observe_with(
    Source::function("database::on-row-changed")
        .args(json!({ "db": "primary", "table": "orders", "ops": ["insert", "update"] }))
        .capture([Capture::Input, Capture::Caller]),
    "orders::project",
    ObserverOptions::default().max_in_flight(1),         // the third argument
);

let route = iii.observe(
    Source::function("http::on-request").args(json!({ "method": "GET", "path": "/users/:id" })),
    "users::get",
);

let inv = iii_sdk::current_invocation();   // Option<InvocationCtx>
// iii.trigger(TriggerRequest::new(id, payload).dispatch(Dispatch::Remote)) forces the engine path
```

### Python

```python
digest = iii.observe(
    {"function_id": "cron::on-tick",
     "args": {"expression": "0 0 9 * * Mon-Fri"},
     "filters": {"input.scheduled_time": {"$gte": "2026-11-01T00:00:00Z"}}},
    "local::cron-handler",
    {"max_fires": 10},                    # optional third argument
)
await digest.ready_async()                # ready() blocks in sync code
```

### Go

```go
digest, err := client.Observe(iii.Source{
    FunctionID: "cron::on-tick",
    Args:       iii.Args{"expression": "0 0 9 * * Mon-Fri"},
    Filters:    iii.Filters{"input.scheduled_time": iii.Op{"$gte": "2026-11-01T00:00:00Z"}},
}, "local::cron-handler")                 // optional: iii.ObserverOptions{MaxFires: 10, TargetNamespace: …}
err = digest.Ready(ctx)
inv, ok := iii.InvocationFromContext(ctx)
// client.IsObserved(id) / client.Observers(id) are the FunctionRef.isObserved() / observers() of Go
```

## 3. `trigger()` does not change for callers

There is no new call API. An optional `dispatch?: 'auto' | 'remote'`, default
`auto`, forces the engine path. Tests use it, and so do functions whose callers
rely on engine-side effects.

The SDK takes the **local path** when every one of these holds:

1. the function is registered in this SDK instance with a handler (HTTP-invoked
   functions have none: `N:561`, `R:1232`) and `localDispatch` is not `false`;
2. the engine confirmed it with `functionregistered` and `local_dispatch: true`;
3. the resolved namespace equals the worker's namespace. The local map is keyed
   by bare id, and `engine::*` / `configuration::*` resolve to `default`
   (`N:147-156`, `R:743-759`);
4. `action` is not `enqueue` (the caller expects a queue receipt). `void` runs
   locally, fire-and-forget;
5. `dispatch !== 'remote'`;
6. the call is nested at most 64 local calls deep in the same task or async
   context (re-entrancy guard). A deeper call takes the remote path, so
   recursive self-calls cannot overflow the stack.

Otherwise the call goes over the wire as today.

Differences from the remote path, all documented in the upgrade guide:
nothing is serialized, so a local call has **no 16 MiB frame limit** (a test
that asserts `payload_too_large` on a call to a function hosted by the same
instance must force `dispatch: 'remote'`); the engine records no `call` span
or metrics for it (counts arrive through `localinvocationstats`); the handler
gets a copy of the payload and the caller a copy of the result.

## 4. Internals

### 4.1 Per-function state

The function map entry (`N:258`, `R:1314`, `PY:268`) gains:

```ts
hooks?: ObserveHooks                  // SDK-local callbacks
confirmed: boolean                    // set by functionregistered; false initially
localDispatch: boolean                // functionregistered.local_dispatch ∧ option
revision: number
observers: Map<id, Observer>          // as acked; the source of isObserved()/observers()
compiled: CompiledSet                 // [{ id, match(doc), capture, on }], needs: {input, output, caller, metadata};
                                      // auto: evaluated per local call; owner: looked up by id in emit / call
```

- `isObserved()` reads `compiled.length > 0`: one load, for both modes.
- Swapping `compiled` is one reference assignment: `ArcSwap` in Rust, plain
  assignment in Node. The hot path reads it once per invocation.
- An observer enters `compiled` **before** the ack is sent, so the first
  invocation (auto) or emit (owner) after the engine marks it `active` is
  already evaluated.
- `RegistrationRejected{FUNCTION_NAMESPACE_CONFLICT}` clears `confirmed` and
  `localDispatch` and drops the set. Today it is only logged while the handler
  keeps serving locally (`N:1158-1162`, `R:2635-2644`).

### 4.2 Inbound observer messages

```
on functionregistered(fn, rev, list):
  fn.confirmed = true; fn.localDispatch = msg.local_dispatch ∧ option
  diff list against fn.observers:
    for removed ids → onObserverRemoved(obs)        // e.g. after an engine restart
    for new ids     → handleAdded(obs, rev)
    for kept ids    → ack again with rev, without calling the hook (the engine holds a replayed set
                      pending until acked); a stored late rejection is sent as that ack's error
                      instead. An observer whose args changed is removed, then added.
  fn.revision = rev

on observeradded(fn, rev, obs):  if rev <= fn.revision on this connection → ignore; else handleAdded(obs, rev)
on observerremoved(fn, rev, id): if rev <= fn.revision → ignore; else remove from compiled, onObserverRemoved(obs)

handleAdded(obs, rev):                              // serialized per function
  await hooks.onObserverAdded?(obs)                 // throw → ack {error}; timeout 30 s → ack {error: OWNER_TIMEOUT}
                                                    // an owner indexes obs here (job, route, table)
  if obs has a stored rejection (reject() ran meanwhile): send observerack { …, error } and stop   // a rejection wins over activation
  compiled += { id, match: compile(obs.filters), capture, on }   // before the ack
  send observerack { function_id, id, revision: rev }            // rev: what the engine announced for this observer
  fn.revision = rev
```

### 4.3 The local call

```
invokeLocal(fn, req):
  if local nesting depth >= 64: take the remote path     // re-entrancy guard
  set      = fn.compiled                                 // captured at call start; empty for owner functions
  parent   = currentInvocation()                         // ALS / task_local / contextvar / ctx
  ctx      = { function_id: fn.id, invocation_id: uuid(),
               caller: { worker_id: self.workerId, worker_name: self.workerName,
                         function_id: parent?.function_id, invocation_id: parent?.invocation_id,
                         parent: strip(parent?.caller) },
               observe_depth: parent?.observe_depth ?? 0 }
  input    = clone(req.payload)                          // value semantics, README §11.8
  snapshot = set non-empty ? clone(req.payload) : none   // what a report carries: the payload as the caller sent it
  started  = now()
  outcome  = run fn.wrapper(input, req.metadata) within ctx   // reuses the existing wrapper, so the
                                                             // `execute <fn>` span is still created
  (void → don't await; otherwise race the timeout → InvocationError{TIMEOUT}, same as remote)
  errors   → the same InvocationError shape as the remote path
  stats.count(fn.id, outcome)
  if set is non-empty:                                   // the set captured at call start: if it was empty the call
                                                         // is not reported and its input was never cloned for reporting
      doc     = lazy view { input, output | error, caller, metadata }
      matched = [o.id for o in set if o.on accepts outcome and o.match(doc)]
      if matched: reporter.push(item(fn, ctx, outcome, union of matched captures))   // never blocks
  return clone(outcome)
```

For owner functions the local path is the same minus the automatic report:
the handler itself picks observers and calls `obs.emit()` or `obs.call()`.

```
emit(obs, event):                                        // owner functions only, else NOT_OWNER_DELIVERED
  c       = fn.compiled[obs.id]                          // absent (removed, rejected) → return false, nothing sent
  cur     = currentInvocation()
  caller  = cur?.function_id == fn.id ? cur.caller       // inside the function's own handler: that invocation
          : cur ? { function_id: cur.function_id, invocation_id: cur.invocation_id, parent: strip(cur.caller) }
          : {}                                           // outside any handler; the engine stamps worker_*
  doc     = lazy view { input: event, caller, metadata: cur?.metadata }
  if !c.match(doc): return false                         // nothing sent
  reporter.push(item(fn, cur, event, caller, matched: [obs.id], captures of c))   // carries cur's traceparent / baggage
                                                         // same event value, still in the buffer → one item, matched: [ids…]
  return true

call(obs, payload):                                      // owner functions only
  if !fn.compiled[obs.id].match({ input: payload, caller, metadata }): reject FILTERS_NOT_MATCHED   // nothing sent
  remote trigger(obs.target, payload, activation: { observer_id: obs.id }, caller, observe_depth)
```

**Insertion points**
- Node: right after namespace resolution in `trigger()` (`N:643`), reusing the wrapper built at `N:495-559`.
- Rust: `R:1954`, copying the lookup at `R:2741-2746` and the span setup at `R:2793-2813`.
- Python: `PY:1575`.
- Go: `GO:488`.

**Current-invocation context.** No SDK has one today; this is new work.
- Node: an `AsyncLocalStorage`, set in `onInvokeFunction` (`N:1276`) from the inbound `InvokeFunction.caller` / `activation` / `observe_depth`, and in `invokeLocal`.
- Browser: no `AsyncLocalStorage`. A synchronous scope set around the handler call and restored when it returns, plus `iii.withInvocation(inv, fn)` to re-enter a captured context. Past an `await`, the ambient depth is that of the deepest running invocation and the caller is attached only when unambiguous (§2).
  Outside a scope the browser estimates call depth from the running invocations: calls made in separate macrotasks while earlier local handlers still run may be counted as nested (past 64 levels some go remote while connected); offline, hosted functions run locally up to 256 levels, then fail with `LOCAL_DEPTH_EXCEEDED`.
- Rust: a `tokio::task_local!`, set around the handler future in `handle_invoke_function` (`R:2729`) and in the local path. Code that `tokio::spawn`s from inside a handler loses it; `iii_sdk::scope(ctx, fut)` carries it explicitly.
- Python: a `contextvars.ContextVar`. Sync handlers run on a raw `threading.Thread` (`PY:1481-1494`), so they must start with `contextvars.copy_context().run`, which also fixes OTel context loss there today.
- Go: a `context.Context` value, which the handler already receives.

Outgoing remote `trigger()` calls add `caller: {function_id, invocation_id, parent}` and `observe_depth` from the context, only when the engine advertised `observe.v1`. Report and emit callers include the `worker_id` / `worker_name` the SDK knows; the engine overwrites them.

**Execution context changes.**
- In Rust, remote handlers run on the iii connection thread's runtime (`R:2787`). Local ones run on the caller's task, and a sync handler (`RegisterFunction::new`) runs inline. A function whose sync handler can block should be async or opt out (`localDispatch: false`).
- In Python, `trigger_async` assumes the SDK loop. The local path must run on the caller's loop or thread instead.

### 4.4 The reporter

- **One buffer for both modes.** It carries matched local calls of auto functions and `obs.emit` events of owner functions; the engine handles them the same way ([`protocol.md`](protocol.md) §3.4).
- **Bounded.** The default is 10 000 items or 8 MiB, and the 8 MiB covers the bytes already flushed but not yet written to the socket (Node `ws.bufferedAmount`, Rust bytes in the outbound channel or held frames, Python and Go their send queue). When full it drops the newest and counts it under `BUFFER_FULL` (or defers the flush), so a peer that stops reading cannot grow memory without bound. It never blocks the invocation or the emit.
- **Flushing.** At 256 items or after 5 ms linger, whichever comes first. A flush is split to stay under the 16 MiB frame limit (`N:1041-1064`, `R:2063-2124`). An item too large on its own is dropped and counted as `TOO_LARGE`.
- **Held until confirmed.** Items of a function are held until `functionregistered` arrived for it on the current connection. A function that cannot be confirmed (`FUNCTION_NAMESPACE_CONFLICT`, unregister) has its held items discarded and counted as `INACTIVE`, and an item pushed after the function was released is counted `INACTIVE`, never held. Held items of a function confirmed on the current connection are never expired or discarded while its hooks are still running.
- **Coalescing.** An emit item enters the coalescing index only while it is in the buffer; an item already flushed is never merged into.
- **While disconnected.** Items stay in the same bounded buffer. What happens to them is decided at flush time, per connection: nothing is sent unless the engine of the new connection advertised `observe.v1`, and items are sent once their function is confirmed on it. Observe-family frames queued while disconnected (`observe`, `unobserve`, `observerack`, `invocationsobserved`, `localinvocationstats`) are dropped at flush time under the same rule. The engine delivers the items whose observers are active when they arrive; the rest are dropped as `INACTIVE`.
- **Stats.** Drop counters and per-function local invocation counts go out in `localinvocationstats` every 10 s, only when non-zero. The counts include every local invocation, observed or not, because they feed the engine's invocation metrics. "Zero wire" therefore means no per-call frames: it excludes this periodic frame.

### 4.5 Reconnect

- **Local dispatch survives a disconnect.** Once confirmed, it stays enabled, so
  a worker keeps serving calls to its own functions instead of queueing them
  behind a dead socket.
- **The observer set is replaced only by the engine.** On reconnect the
  replayed `RegisterFunction` gets a fresh `functionregistered` with the full
  set, and the SDK diffs it (§4.2): `onObserverRemoved` for what disappeared,
  `onObserverAdded` for what is new, and a plain re-ack for what it already
  holds. A late rejection (`obs.reject`) is stored and re-sent here. This is safe: the engine never marks an
  observer `active` before the owner acked it, so a stale set cannot miss an
  active observer.
- **What gets replayed.** Functions (with their `observe` descriptor) and
  observers owned by this SDK (`observe` with the same id), functions
  first, so local targets exist before the observers that name them. The replay
  points are `N:988-996`, `R:2365-2390`, `PY:499-551` and `GO:819-857`. With a
  valid `Reattach`, the engine re-parents the observers and the replay is a
  no-op that keeps `fires` counters.

## 5. Inbound message handling

New branches for `functionregistered`, `observeradded`,
`observerremoved`, `observeresult` and `observerstatechanged`. They go in:

- Node: `MessageType` `NT:16-29`, `onMessage` `N:1411-1457`;
- Rust: `RP:139` plus the match at `R:2512-2616`;
- Python: `PYT:10-25` and `PY:681-715`;
- Go: `GOP:35-50` and `GOP:389-441`.

Unknown fields and message types are ignored. The SDK advertises `features` in the `engine::workers::register` payload:
Node `N:715-734`; Rust `WorkerMetadata`, `R:575-599`; Python `PY:1674-1725`;
Go `GO:894-902`.

## 6. Deprecations

| Today | Proposed | Phase |
|---|---|---|
| `registerTriggerType(type, handler)` | `registerFunction(id, handler, { observe: hooks })` | deprecated 5, removed 6 |
| `TriggerHandler.registerTrigger(config)` | `ObserveHooks.onObserverAdded(obs)` | 5 / 6 |
| provider fan-out: `iii.trigger(binding.function_id, payload)` per binding | `obs.emit(event)` (or `obs.call(payload)`) per observer the owner routes to | 5 / 6 |
| `TriggerHandler.unregisterTrigger(config)` | `ObserveHooks.onObserverRemoved(obs)` | 5 / 6 |
| `TriggerConfig {id, function_id, config, metadata}` | `Observer {id, args, filters, target}`; metadata stays engine-side | 5 / 6 |
| `trigger_request_format` | `observe.args_format` | 5 / 6 |
| `call_request_format` | `request_format` of the function | 5 / 6 |
| `registerTrigger({type, function_id, config, metadata})` | `observe({ function_id, args, filters }, target, { metadata })` | deprecated 5, removed 6 |
| `TriggerTypeRef.registerTrigger / registerFunction` helpers | `observe` | 5 / 6 |
| `unregisterTriggerType` | engine now decodes and honors it (0.25); the SDKs keep sending `{id, namespace?}`; language marker only, removed in phase 6 | 0 |
| Rust `RegisterTriggerType::new(..)`, `iii.register_trigger(RegisterTriggerInput{..})` | `RegisterFunction::observe(..)`, `iii.observe(Source::function(..).args(..), target)` | 5 / 6 |

- `registerTrigger` and `registerTriggerType` (also through trigger-type helper
  objects) log one warning per process per API in phase 5, pointing to the
  migration guide, and keep working through either a live old-style provider
  or the engine legacy shim ([`protocol.md`](protocol.md) §6). The other
  deprecated names carry the language-level marker only
  (`@deprecated`, `#[deprecated(since = "0.25.0")]`, `DeprecationWarning`
  docs, `// Deprecated:`).
- `iii.interest()` from the previous draft of this plan is dropped: hooks plus
  `FunctionRef.isObserved()` / `observers()` cover it.

## 7. SDK test plan

- **Filter conformance.** Run `sdk/fixtures/filters/**` in every SDK's CI.
- **Equivalence.** Run every auto-delivery test with `dispatch: 'auto'` and
  again with `'remote'`. The observer-visible envelopes must be identical, apart
  from `invocation_id` / `activation_id` and timings.
- **Zero-wire.** Install a frame counter on the socket. 1M local calls of an
  unobserved function must send zero per-call frames: the counter excludes the
  periodic `localinvocationstats` (drive its timer with fake or paused time),
  and it must see sends made during the loop (yield periodically, or count
  synchronously at the socket write). Include a positive control: an observed
  function in the same harness produces a non-zero count.
- **Activation race.** Call `observe()`, await `ready()`, then make N local
  calls. Exactly N activations must arrive (with `max_in_flight` large enough).
- **Args.** `obs.args` equals what the observer sent; a missing required
  argument and args on a function without `args_format` fail with
  `INVALID_ARGS` before `onObserverAdded` runs; `args_format` on an auto function
  throws `INVALID_OBSERVE_DESCRIPTOR` at `registerFunction`.
- **Routing.** Two observers with different args on one owner function only
  receive what the owner emits to them; an emit right after `ready()` resolves
  is delivered (the owner indexed the observer before the ack).
- **Hooks.** `onObserverAdded` runs once per add and before the ack; a throw yields
  `error` with the rejection code on the observer's `ready()`;
  `engine::observers::get` shows the observer's filters unchanged;
  `onObserverRemoved` runs on unobserve, dispose, observer grace expiry and on a
  reconnect diff.
- **Targets.** An observer whose target is not registered stays `pending`
  and turns `active` once it is; a target equal to the observed function is
  `SELF_TARGET`.
- **Owner delivery.**
  - `obs.emit()` returns `false` and sends no frame when the filters reject
    the event; otherwise the target receives the envelope with the caller rules
    of §1 (inside the own handler, another handler, no handler); emits of one
    event to N observers in one flush are one report item.
  - `obs.call()` returns the target's result; it rejects with
    `FILTERS_NOT_MATCHED` and no frame when the filters reject the payload, and
    with `OBSERVER_NOT_ACTIVE` after dispose; `obs.emit()` after dispose
    returns `false` without throwing.
  - `obs.reject()` removes the observer from the compiled set at once, is
    re-sent after a reconnect with the last announced revision, and wins over
    a still-running `onObserverAdded`.
  - Both throw `NOT_OWNER_DELIVERED` on an auto function; the target sees the
    observer's metadata sidecar.
- **Reconnect.** Drop the socket mid-stream, then check that local dispatch
  keeps serving, reports flush after reconnect, the `functionregistered` diff
  calls the right hooks, `Reattach` keeps `fires`, and a
  `FUNCTION_NAMESPACE_CONFLICT` on replay disables local dispatch.
- **Value semantics.** A handler that mutates its input must not affect the
  caller's object, and the reverse.
- **Local limits.** A local call has no 16 MiB limit (force `dispatch: 'remote'`
  to test `payload_too_large`), and nesting deeper than 64 falls back to the
  remote path.
- **Listeners and hooks.** `onStateChange` listeners run asynchronously, in
  order, never on the read loop; hooks of one function may overlap after an
  `OWNER_TIMEOUT`.
