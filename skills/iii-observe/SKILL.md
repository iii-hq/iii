---
name: iii-observe
description: >-
  Use when one iii function should run when another runs (iii.observe), when binding functions to
  HTTP routes, cron schedules, queue topics, state or database changes, or engine events through
  their source functions (http::on-request, cron::on-tick, queue::on-message, state::on-changed,
  database::on-row-changed), when writing filters, or when writing an event source with
  onObserverAdded, onObserverRemoved, and onObserverDeliver across TypeScript, browser, Python,
  Rust, and Go.
---

# Observe

`iii.observe(a, b)`: when `a` runs, `b` runs.

```typescript
iii.observe("orders::create", "alerts::big-order");
```

Every function is observable. By default, each successful call of `orders::create` runs
`alerts::big-order` with an envelope of that call (its input, output, and caller). Events are
function calls too: an HTTP request is a call of `http::on-request`, a cron tick a call of
`cron::on-tick`, a row change a call of `database::on-row-changed`. Observing one of them is how a
function runs on that event.

| Part | Fields | Notes |
| --- | --- | --- |
| `source` (left side) | `"fn::id"`, or `{ function_id, namespace?, args?, filters?, on?, capture?, deliver? }` | `args` are for the function's worker (a route, a cron expression, a topic), validated against its `args_format`. `filters` are for iii: they select which calls reach the target. `on`: `success` (default), `error`, `complete`. `capture`: default `['input', 'output', 'caller']`, `[]` allowed. `deliver`: `envelope` (default), `input`, `output` |
| `target` (right side) | a registered function id (or `{ function_id, namespace }`) | No inline handler. The observer stays `pending` until the target is registered |
| `options` | `id?`, `max_fires?`, `expires_in_ms?`, `max_in_flight?`, `metadata?` | `id` defaults to a UUID kept across replays. `metadata` is the sidecar of every activation |

The source namespace resolves like `call()`: the worker's namespace, except `engine::*` and
`configuration::*`, which resolve in `default`.

## Observe a function

Register the target, then observe. This alerts on failed charges of 500 or more:

```typescript
import { registerWorker, type Observed } from "iii-sdk";

const iii = registerWorker(process.env.III_URL ?? "ws://localhost:49134");

iii.registerFunction("alerts::payment-failed", async (event: Observed<{ amount: number }>) => {
  console.log(event.function_id, event.error?.code, event.input?.amount, event.caller?.worker_name);
  return null;
});

const failures = iii.observe(
  {
    function_id: "payments::charge",
    on: "error",
    filters: { "input.amount": { $gte: 500 } },
    capture: ["input", "error", "caller"],
  },
  "alerts::payment-failed",
  { max_in_flight: 4 },
);
await failures.ready(); // rejects with ObserveRejection (error.code) on error or disposed
// later: await failures.dispose()
```

```python
iii.register_function("alerts::payment-failed", lambda event: print(event["error"]["code"]))

failures = iii.observe(
    {"function_id": "payments::charge", "on": "error",
     "filters": {"input.amount": {"$gte": 500}}, "capture": ["input", "error", "caller"]},
    "alerts::payment-failed",
    {"max_in_flight": 4},
)
failures.ready()  # blocks; in async code: await failures.ready_async()
```

```rust
use iii_sdk::{Capture, Error, ObserverOptions, Observed, On, RegisterFunction, Source};
use serde_json::{json, Value};

iii.register_function(
    "alerts::payment-failed",
    RegisterFunction::new(|event: Observed<Value>| {
        println!("{} failed: {:?}", event.function_id, event.error);
        Ok::<_, Error>(())
    }),
);
let failures = iii.observe_with(
    Source::function("payments::charge")
        .on(On::Error)
        .filters(json!({ "input.amount": { "$gte": 500 } }))
        .capture([Capture::Input, Capture::Error, Capture::Caller]),
    "alerts::payment-failed",
    ObserverOptions::default().max_in_flight(4),
);
failures.ready().await?;
```

```go
client.RegisterFunction("alerts::payment-failed", func(ctx context.Context, data json.RawMessage) (any, error) {
	var ev iii.Observed
	if err := json.Unmarshal(data, &ev); err != nil {
		return nil, err
	}
	log.Printf("%s failed: %s", ev.FunctionID, ev.Error.Code)
	return nil, nil
})
failures, err := client.Observe(iii.Source{
	FunctionID: "payments::charge",
	On:         iii.OnError,
	Filters:    iii.Filters{"input.amount": iii.Op{"$gte": 500}},
	Capture:    []iii.Capture{iii.CaptureInput, iii.CaptureError, iii.CaptureCaller},
}, "alerts::payment-failed", iii.ObserverOptions{MaxInFlight: 4})
if err == nil {
	err = failures.Ready(ctx) // *iii.ObserveRejectionError carries the code
}
// shorthand for a bare function id: client.ObserveFunction("orders::create", "alerts::big-order")
```

The browser SDK (`iii-browser-sdk`) has the same `observe` API as Node.

## Run a function on an event

Each capability worker hosts a source function. Add the worker once
(`iii call compose::add worker=http`), then observe its function with the args it documents:

| Source function | Worker | `args` | What the target receives |
| --- | --- | --- | --- |
| `http::on-request` | `http` | `{ method, path, middleware_function_ids?, public_webhook? }` | The request `{ method, path, path_params, query_params, headers, body }`. Its result is the HTTP response `{ status_code, headers?, body? }` |
| `cron::on-tick` | `cron` | `{ expression }` (7 fields: sec min hour day month weekday year) | The envelope; `input` is `{ expression, scheduled_time, actual_time }` |
| `queue::on-message` | `queue` | `{ topic, max_retries?, backoff_ms?, queue_config?, group? }` | The published data. Returning acks the message; an error retries it with backoff, then moves it to the topic's DLQ |
| `state::on-changed` | `state` | none; filter on `input.scope`, `input.key` | The envelope; `input` is `{ event_type, scope, key, old_value, new_value }` |
| `database::on-row-changed` | `database` | `{ db, table?, ops? }` | The envelope; `input` is the row change (`db`, `table`, `op`, ...) |
| `pubsub::publish` | `pubsub` | none; filter on `input.topic` | The envelope; `input` is `{ topic, data }` |

Other workers name their sources `<worker>::on-<event>` and list them in their README. Engine
sources are [below](#engine-hosted-sources).

```typescript
iii.registerFunction("orders::create", async (req: { body: { sku: string; qty: number } }) => {
  const order = await saveOrder(req.body); // your code
  return { status_code: 201, body: order };
});

// POST /orders runs orders::create; its result is the response
iii.observe({ function_id: "http::on-request", args: { method: "POST", path: "/orders" } }, "orders::create");

// every day at 09:00; deliver: 'input' sends the tick alone
iii.observe(
  { function_id: "cron::on-tick", args: { expression: "0 0 9 * * * *" }, deliver: "input" },
  "reports::daily-summary",
);

// each message published to the "payments" topic
iii.observe({ function_id: "queue::on-message", args: { topic: "payments", max_retries: 5 } }, "payments::settle");

// each change in the "todos" state scope
iii.observe(
  { function_id: "state::on-changed", deliver: "input", filters: { "input.scope": { $eq: "todos" } } },
  "todos::on-change",
);
```

```python
iii.observe({"function_id": "http::on-request", "args": {"method": "POST", "path": "/orders"}},
            "orders::create")
iii.observe({"function_id": "cron::on-tick", "args": {"expression": "0 0 9 * * * *"},
             "deliver": "input"}, "reports::daily-summary")
```

```rust
iii.observe(
    Source::function("http::on-request").args(json!({ "method": "POST", "path": "/orders" })),
    "orders::create",
);
iii.observe(
    Source::function("cron::on-tick")
        .args(json!({ "expression": "0 0 9 * * * *" }))
        .deliver(Deliver::Input),
    "reports::daily-summary",
);
```

```go
client.Observe(iii.Source{
	FunctionID: "http::on-request",
	Args:       iii.Args{"method": "POST", "path": "/orders"},
}, "orders::create")
client.Observe(iii.Source{
	FunctionID: "cron::on-tick",
	Args:       iii.Args{"expression": "0 0 9 * * * *"},
	Deliver:    iii.DeliverInput,
}, "reports::daily-summary")
```

- Use leading slashes in `path` (`/orders`, `/users/:id`) and `expression` for cron.
- One observer of `http::on-request` owns a method and path; a second one for the same route is
  rejected with `HTTP_ROUTE_CONFLICT`. `middleware_function_ids` run before the target.
- `queue::on-message` observers that share a `group` handle each message once; every other
  observer of the topic receives every message.
- Publish to a queue topic with `iii.call({ function_id: "iii::durable::publish", payload: { topic,
  data } })`.
- Event sources such as `http::on-request` and `cron::on-tick` are `private`: only their worker
  calls them, and a call from anywhere else fails with `FORBIDDEN`.

## What the target receives

- **Envelope** (default `deliver`): `{ observer_id, activation_id, function_id, namespace,
  invocation_id, status, input?, output?, error?, caller?, metadata?, started_at_ms, duration_ms }`.
  Only captured fields are present. `deliver: 'input'` or `'output'` sends that value alone.
- The observer's `options.metadata` arrives as the handler's metadata argument (Node and browser
  second argument, Python `metadata` parameter, the `Option<Value>` of a Rust
  `Fn(Value, Option<Value>)` handler, Go `iii.MetadataFromContext(ctx)`).
- iii's delivery is fire-and-forget and at most once: the target's result is discarded, and drops
  are counted. A source whose worker calls targets itself documents what its target receives and
  what its result does (an `http::on-request` target returns the response; a `queue::on-message`
  target's result acks the message).

## Handle and states

`pending` (no host, no ack, or no target yet) → `active` or `error`. `suspended` while the target
is away, or while the observing worker is disconnected (30 s grace, then `disposed`). `disposed`
after `dispose()`, `max_fires`, or expiry. `observe()` returns synchronously, works before the
connection is up, and is replayed with the same id on reconnect. `onStateChange(listener)` (Python
`on_state_change`, Go `OnStateChange`, Rust `on_state_change`) reports every transition with a
reason code.

## Filters

Filters are declarative JSON, evaluated by the same `iii-filter` language in the engine and every
SDK. Full reference: https://iii.dev/docs/reference/filters.

- A clause is `"<path>": { "$op": operand }`. Explicit operators only: `{ "input.op": "insert" }`
  is `INVALID_FILTER`; write `{ "input.op": { "$eq": "insert" } }`. Clauses are ANDed.
- Roots: `input`, `output`, `error`, `caller`, `metadata`. A filter may read fields it does not
  capture.
- Operators: `$eq $ne $in $nin $gt $gte $lt $lte $exists $prefix $any`; combinators `$and $or
  $not`. `$any` matches array elements (operator object, or a filter over `@`).
- Only `$ne`, `$nin` and `$exists: false` match an absent path. `null` is a value. No type
  coercion; arrays need `$any` to match an element.
- Limits: depth 8, 64 nodes, 256 `$in` items, 16 path segments, 8 KiB as written.
- Paths are checked against the function's `request_format` / `response_format`: an unknown
  property under a closed schema is `INVALID_FILTER_PATH`.

```json
{ "caller.parent.worker_name": { "$ne": "search-indexer" },
  "input.items": { "$any": { "@.sku": { "$prefix": "VIP-" } } } }
```

## Write an event source

This is provider-side work: a worker that turns something outside iii (a schedule, a socket, a
database log) into calls of a function others observe. Full guide:
https://iii.dev/docs/creating-workers/observable-functions.

The worker registers a function whose handler returns the event, and calls it when the event
happens. iii does the rest by default. Three hooks override those defaults:

| Hook | What iii does by default | Use the hook to |
| --- | --- | --- |
| `onObserverAdded(obs)` | accepts the observer: it becomes active and receives the function's calls | set something up from `obs.args` before the observer is accepted: start a cron job, add an HTTP route, install a database trigger, open a queue consumer. Throw to reject the observer |
| `onObserverRemoved(obs)` | stops delivering to the observer and forgets it | tear down what `onObserverAdded` set up |
| `onObserverDeliver(call, observers, next)` | after each call, runs the target of every observer whose `on` and filters match | choose which observers get each call and what the caller gets back. `next(call, observers)` runs iii's delivery for the observers passed |

`onObserverAdded` and `onObserverRemoved` exist for one function (by id, or on the `FunctionRef`
that `registerFunction` returns) and for every function the worker hosts. `onObserverDeliver` is
per function. Each method returns its receiver for chaining; passing `null` removes the hook.

| Hook | TypeScript | Python | Rust | Go |
| --- | --- | --- | --- | --- |
| every function | `iii.onObserverAdded(h)` | `@iii.on_observer_added` | `iii.on_observer_added(h)` | `client.OnObserverAdded(h)` |
| one function, by id | `iii.onObserverAdded(id, h)` | `@iii.on_observer_added(id)` | `iii.on_observer_added_for(id, h)` | `client.OnObserverAddedFor(id, h)` |
| one function, on its ref | `ref.onObserverAdded(h)` | `@ref.on_observer_added` | `function.on_observer_added(h)` | `ref.OnObserverAdded(h)` |
| deliver | `iii.onObserverDeliver(id, h)`, `ref.onObserverDeliver(h)` | `ref.on_observer_deliver(h)` | `iii.on_observer_deliver_for(id, h)`, `function.on_observer_deliver(h)` | `client.OnObserverDeliverFor(id, h)`, `ref.OnObserverDeliver(h)` |

`onObserverRemoved` follows the same pattern (`on_observer_removed`, `on_observer_removed_for`,
`OnObserverRemovedFor`). Python hook methods work as decorators and as plain calls; hooks may be
sync or async in Node and Python.

A cron source: one job per distinct expression, and keyed delivery hands each tick to that
expression's observers.

```typescript
import { keyed, ObserveRejection } from "iii-sdk";

type CronTick = { expression: string; scheduled_time: string; actual_time: string };

iii.registerFunction("cron::on-tick", (tick: CronTick) => tick, {
  args_format: { type: "object", required: ["expression"], properties: { expression: { type: "string" } } },
  private: true, // only this worker calls it; every worker may observe it
})
  .onObserverAdded((obs) => {
    const expression = obs.args.expression as string;
    if (!isValidCron(expression)) throw new ObserveRejection("INVALID_CRON_EXPRESSION", expression);
    // your scheduler: one job per expression; each fire is a call of cron::on-tick
    scheduler.retain(expression, (scheduled_time) =>
      iii.call({
        function_id: "cron::on-tick",
        payload: { expression, scheduled_time, actual_time: new Date().toISOString() },
      }),
    );
  })
  .onObserverRemoved((obs) => scheduler.release(obs.args.expression as string))
  .onObserverDeliver(
    keyed({ observer: (o) => o.args.expression as string, call: (c) => c.input.expression }),
  );
```

```python
from iii import ObserveRejection, keyed

cron_args = {"type": "object", "required": ["expression"],
             "properties": {"expression": {"type": "string"}}}
on_tick = iii.register_function("cron::on-tick", lambda tick: tick,
                                 args_format=cron_args, private=True)

@on_tick.on_observer_added
async def start(obs):  # raise ObserveRejection(code, message) to reject
    expression = obs.args["expression"]
    if not is_valid_cron(expression):
        raise ObserveRejection("INVALID_CRON_EXPRESSION", expression)
    scheduler.retain(expression, fire)  # your scheduler; fire() calls cron::on-tick

@on_tick.on_observer_removed
def stop(obs):
    scheduler.release(obs.args["expression"])

on_tick.on_observer_deliver(keyed(observer=lambda o: o.args["expression"],
                                  call=lambda c: c.input["expression"]))

async def fire(expression, scheduled_time):
    await iii.call_async({"function_id": "cron::on-tick", "payload": {
        "expression": expression, "scheduled_time": scheduled_time, "actual_time": now()}})
```

```rust
use iii_sdk::{Call, Error, Keyed, ObserveRejection, Observer, RegisterFunction};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
struct CronArgs { expression: String }

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
struct CronTick { expression: String, scheduled_time: String, actual_time: String }

fn observer_keys(obs: &Observer) -> Vec<String> {
    obs.args::<CronArgs>().map(|a| vec![a.expression]).unwrap_or_default()
}
fn call_keys(call: &Call) -> Vec<String> {
    call.input["expression"].as_str().map(|e| vec![e.to_owned()]).unwrap_or_default()
}

// Set hooks by id before registering, so they see every observer.
let jobs = scheduler.clone(); // your scheduler: one job per expression, each fire calls cron::on-tick
iii.on_observer_added_for("cron::on-tick", move |obs: Observer| {
    let jobs = jobs.clone();
    async move {
        let CronArgs { expression } = obs.args()?; // validated against args_format
        jobs.retain(&expression)
            .map_err(|e| ObserveRejection::new("INVALID_CRON_EXPRESSION", e.to_string()))
    }
});
let jobs = scheduler.clone();
iii.on_observer_removed_for("cron::on-tick", move |obs: Observer| {
    let jobs = jobs.clone();
    async move {
        if let Ok(CronArgs { expression }) = obs.args() { jobs.release(&expression); }
    }
});
iii.on_observer_deliver_for("cron::on-tick", Keyed::new(observer_keys, call_keys));

iii.register_function(
    "cron::on-tick",
    RegisterFunction::new(|tick: CronTick| Ok::<_, Error>(tick))
        .args_format::<CronArgs>()
        .private(),
);
// each fire: iii.call(CallRequest::new("cron::on-tick", serde_json::to_value(&tick)?)).await?;
```

```go
cronArgs := json.RawMessage(`{"type":"object","required":["expression"],"properties":{"expression":{"type":"string"}}}`)
expression := func(args iii.Args) string { e, _ := args["expression"].(string); return e }

// Set hooks by id before registering, so they see every observer.
client.OnObserverAddedFor("cron::on-tick", func(ctx context.Context, obs *iii.Observer) error {
	if !isValidCron(expression(obs.Args)) {
		return iii.ObserveRejection("INVALID_CRON_EXPRESSION", expression(obs.Args))
	}
	return scheduler.Retain(ctx, expression(obs.Args)) // each fire calls cron::on-tick
})
client.OnObserverRemovedFor("cron::on-tick", func(ctx context.Context, obs *iii.Observer) {
	scheduler.Release(expression(obs.Args))
})
client.OnObserverDeliverFor("cron::on-tick", iii.Keyed(
	func(obs *iii.Observer) []string { return []string{expression(obs.Args)} },
	func(call *iii.Call) []string {
		var tick struct{ Expression string `json:"expression"` }
		_ = json.Unmarshal(call.Input, &tick)
		return []string{tick.Expression}
	},
))

_, err := client.RegisterFunction("cron::on-tick", iii.Identity, // returns the tick it is called with
	iii.FunctionOptions{ArgsFormat: cronArgs, Private: true})
// each fire: client.Call(ctx, iii.CallRequest{FunctionID: "cron::on-tick", Payload: tick})
```

How hooks behave:

- **Order.** When an observer is added, the worker-wide `onObserverAdded` runs, then the
  function's. When it is removed, the function's `onObserverRemoved` runs, then the worker-wide
  one. For each function the SDK runs hooks one at a time, in the order the observers arrived.
- **Accepted.** An observer becomes `active` once every `onObserverAdded` that applies has
  resolved, so the cron job or database trigger exists before the first call it should see. A throw
  rejects it: `ObserveRejection` sets the code, any other error is `OBSERVE_REJECTED`, and a hook
  past 30 s is `OWNER_TIMEOUT`. The `onObserverRemoved` of each level that already accepted it then
  runs.
- **Removed.** `onObserverRemoved` runs exactly once for each accepted observer: on `dispose()`, on
  disconnect expiry, and after a late rejection with `obs.reject(code, message)`. An error it throws
  is logged.
- **When hooks take effect.** No observer is acked before the `registerFunction` call returns, so
  a hook set by id before registering, or chained on the returned ref, sees every observer. In Rust
  and Go, set hooks by id before registering. A hook set once observers are active runs for each of
  them at once, and the SDK logs a warning.
- **Delivery.** `next(call, observers)` applies each observer's `on`, filters, policy, and
  `deliver`. A deliver hook that runs a target itself checks the filters with `obs.matches(call)`
  and calls `iii.call({ observer: obs, payload })` (Python `call_async({"observer": obs, ...})`,
  Rust `CallRequest::observer(&obs, payload)`, Go `iii.CallRequest{Observer: obs, ...}`), which
  resolves to the target's result. Only the worker that hosts the function may call through its
  observers (`NOT_FUNCTION_OWNER`).
- **What the caller gets.** The handler's return value is what observers observe and what the
  caller gets back. With `onObserverDeliver`, the caller gets what the hook returns or throws: an
  HTTP source returns the route target's response.
- **`keyed({ observer, call })`** files each observer under its key(s) once, when it is accepted,
  and delivers a call to the observers filed under the call's keys: one hash lookup per key. Keys
  express wildcards by listing them (`"primary|*|insert"`).
- **`isObserved(call?)`** (Python `is_observed`, Rust `is_observed()` / `is_observed_for(&call)`,
  Go `IsObserved`) says whether a call would reach anyone, so a source can skip building events
  nobody wants. `observers()` lists the active ones.
- **`obs.accepted()`** resolves once the SDK has acknowledged the observer. A call through the
  observer made afterwards reaches an active observer, so a source sends initial state this way.
- **Registration options.** `args_format` (JSON Schema for observers' args; `INVALID_ARGS` when
  they fail it). `private: true`: only this worker may call the function, others may observe it,
  and the worker's own calls pass input and result by reference. `observable: false`: nobody may
  observe it (`NOT_OBSERVABLE`), for functions whose payloads carry credentials. `standby: true`:
  when another instance already hosts the id, wait and take over its observers when it leaves. A
  function id has one host per namespace, so hooks run once per observer however many instances
  run.

A worker that should do work only while anything is watching uses the worker-wide hooks:

```typescript
iii.onObserverAdded(() => cdp.retain());
iii.onObserverRemoved(() => cdp.release());
```

## Local dispatch

A worker calling a function it registered itself runs it in-process once the engine confirmed it
(`functionregistered`): no frames when nobody observes it; matching calls are reported in batches.
The handler gets a copy of the payload (a reference, for the worker's own `private` functions),
that path has no 16 MiB limit, and a Rust sync handler runs inline on the caller's task. Opt out
per call (`dispatch: 'remote'`, Rust `.dispatch(Dispatch::Remote)`, Go `Dispatch:
iii.DispatchRemote`), per function (`localDispatch: false`, Python `local_dispatch=False`, Rust
`.local_dispatch(false)`, Go `LocalDispatch: &off`), or engine-wide
(`observers.local_dispatch: false`). Enqueue calls always go through the engine.

## Caller and loops

- `caller` is `{ worker_id, worker_name, function_id?, invocation_id?, parent? }`. `worker_*` is
  stamped by the engine; `parent` is the caller of the handler the call came from, one level only.
  Filter on it to stop echo loops: `{ "caller.parent.worker_name": { "$ne": "indexer" } }`.
- Read the current invocation with `iii.currentInvocation()` (Python `iii.current_invocation()`,
  Rust `iii_sdk::current_invocation()`, Go `iii.InvocationFromContext(ctx)`). In Go pass the
  handler's `ctx` to `Call`; in Rust, code you `tokio::spawn` loses it unless wrapped in
  `iii_sdk::scope(ctx, fut)`; in the browser it ends at the first `await` (use
  `iii.withInvocation(inv, fn)`).
- Every activation is one observe hop deeper. Beyond `observers.max_observe_depth` (default 8) it
  is dropped as `LOOP_GUARD`. A target equal to the observed function is `SELF_TARGET`.

## Engine-hosted sources

Private functions in the `default` namespace whose handler returns the event, so `input` and
`output` are both the event. Observe them with `deliver: 'input'` to receive the event alone.
Calling one is `FORBIDDEN`. The constants live in `EngineSources` (`FUNCTIONS_CHANGED`,
`WORKERS_CHANGED`, `OBSERVERS_CHANGED`, `LOG`, `SPAN`, `CONFIGURATION_UPDATED`, `STREAM_CHANGED`,
`STREAM_JOIN`, `STREAM_LEAVE`).

| Function | `input` |
| --- | --- |
| `engine::on-functions-changed` | `{ event: "functions_changed", functions: [...] }`, bursts coalesced (~100 ms) |
| `engine::on-workers-changed` | `{ event, worker_id }`, `event` one of `worker_connected`, `worker_disconnected`, `worker_metadata_updated` |
| `engine::on-observers-changed` | `{ observer_id, function_id, target, state, reason? }`, one call per state change |
| `configuration::on-updated` | `{ type, event_type, id, name, ..., old_value, new_value }` |
| `observability::on-log` | one log record |
| `observability::on-span` | `{ trace_ids: [...] }` per window (~300 ms) |
| `stream::on-changed`, `stream::on-join`, `stream::on-leave` (`iii-stream`) | the stream event; filter on `input.stream_name`, `input.group_id` |

```typescript
iii.observe({ function_id: "engine::on-workers-changed", deliver: "input" }, "ops::on-worker");
iii.observe(
  { function_id: "configuration::on-updated", deliver: "input",
    filters: { "input.id": { $eq: "orders" }, "input.event_type": { $eq: "configuration:updated" } } },
  "orders::reload",
);
// a worker in another namespace names default for observability::* and stream::*
iii.observe(
  { function_id: "observability::on-log", namespace: "default", deliver: "input",
    filters: { "input.severity_text": { $eq: "ERROR" } } },
  "alerts::log",
);
```

## Code written for 0.25

To move code written for 0.25 to 0.26, follow the upgrade guide:
https://iii.dev/docs/upgrading/from-0-25-x.

## Error codes

| When | Codes |
| --- | --- |
| Creating an observer (`ready()` rejects) | `UNSUPPORTED` (engine older than 0.25, or an unknown `on`/`capture`/`deliver`), `INVALID_ARGS`, `INVALID_FILTER`, `INVALID_FILTER_PATH`, `NOT_OBSERVABLE`, `FORBIDDEN`, `SELF_TARGET`, `LIMIT_EXCEEDED`, `OBSERVER_ID_CONFLICT`, `OWNER_TIMEOUT`, `OBSERVE_REJECTED`, or the hosting worker's own code (`HTTP_ROUTE_CONFLICT`, `INVALID_CRON_EXPRESSION`, ...) |
| Registering a function | `INVALID_ARGS_FORMAT`, `FUNCTION_NAMESPACE_CONFLICT` (another connection hosts the id; see `standby`), `UNSUPPORTED` (a function with `args_format`, `private`, `observable: false` or `onObserverDeliver` on an engine without `observe.v2`: the SDK logs it once and registers the function when it connects to an engine that has it) |
| Calling a private function from another worker | `FORBIDDEN` |
| A call through an observer | `NOT_FUNCTION_OWNER`, `OBSERVER_NOT_FOUND`, `OBSERVER_NOT_ACTIVE`, `OVERFLOW`, `LOOP_GUARD`, `TIMEOUT`, or the target's error |
| State reasons | `OWNER_UNAVAILABLE`, `TARGET_UNAVAILABLE`, `OBSERVER_DISCONNECTED`, `EXPIRED`, `MAX_FIRES`, `UNOBSERVED` |
| Drop reasons (metrics) | `INACTIVE`, `LOOP_GUARD`, `MAX_FIRES`, `OVERFLOW`, `REPORTER_NOT_OWNER`, `CANCELLED`, `INVALID_REPORT`, `BUFFER_FULL`, `TOO_LARGE` |

Inspect observers with `engine::observers::list({ function_id?, namespace?, worker? })` and
`engine::observers::get({ id })`; `engine::observers::dispose({ id })` removes one. Docs:
https://iii.dev/docs/using-iii/observers.

## When to Use

- Use this skill to run a function when another function is called, fails, or completes; to bind
  a function to an HTTP route, a cron schedule, a queue topic, a state or database change, or an
  engine event; or to write or debug filters.
- Use it to write an event source other workers observe, with `onObserverAdded`,
  `onObserverRemoved`, `onObserverDeliver`, `keyed`, and `private`.
- Use it when code written for 0.25 needs to move to 0.26.

## Boundaries

- For registering functions, calling them (`call`, void, enqueue), workers, and the registry, use
  `iii-core-primitives`.
- For the `observers:` engine config section and RBAC hooks, use `iii-engine-config`.
- For package exports and language syntax, use `iii-sdk-reference`; for error handling in general,
  use `iii-error-handling`.
- A capability worker's source function, its `args`, and what its target receives are documented
  by that worker. Use the ids in this skill and the worker's README; do not invent others.
