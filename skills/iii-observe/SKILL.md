---
name: iii-observe
description: >-
  Use when one iii function should run when another runs (iii.observe), when binding functions to
  HTTP routes, cron schedules, queue topics, state or database changes, or engine events through
  their source functions (http::on-request, cron::on-tick, queue::on-message, state::on-changed,
  database::on-row-changed), when writing filters, or when writing an event source with
  onObserverAdded, onObserverRemoved, onObserverMatch, and onObserverDeliver across TypeScript,
  browser, Python, Rust, and Go.
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
| `source` (left side) | `"fn::id"`, or `{ function_id, namespace?, args?, filters?, on?, capture?, deliver? }` | `args` are for the function's worker (a route, a cron expression, a topic), validated against its `args_format`. `filters` are for iii: they select which calls reach the target. `on`: `success` (default), `error`, `complete`. `capture`: which parts of the call the envelope carries (`input`, `output`, `error`, `caller`, `metadata`); default `['input', 'output', 'caller']`; ids, status, and timing are always there, and `capture: []` (for an observer that counts calls or records errors) moves no payloads. `deliver`: `envelope` (default), `input`, `output` |
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
| `publish`         | `pubsub` | none; filter on `input.topic` | The envelope; `input` is `{ topic, data }` |

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
happens. iii does the rest by default. Four hooks override those defaults:

| Hook | What iii does by default | Use the hook to |
| --- | --- | --- |
| `onObserverAdded(obs)` | accepts the observer: it becomes active and receives the function's calls | set something up from `obs.args` before the observer is accepted: start a cron job, add an HTTP route, install a database trigger, open a queue consumer. Throw to reject the observer |
| `onObserverRemoved(obs)` | stops delivering to the observer and forgets it | tear down what `onObserverAdded` set up |
| `onObserverMatch(obs, call)` | every observer wants every call | say which calls each observer wants, from its args: a predicate `(obs, call) => boolean`, or `{ observer, call }` keys that iii indexes |
| `onObserverDeliver(call, observers)` | runs the target of every matching observer whose `on` and filters match the call, and gives the caller the handler's return value | deliver each call yourself: the hook receives the matched observers, and what it returns or throws is what the caller gets back |

`onObserverAdded` and `onObserverRemoved` exist for one function (by id, or on the `FunctionRef`
that `registerFunction` returns) and for every function the worker hosts. `onObserverMatch` and
`onObserverDeliver` are per function. A function can set both: its deliver hook receives the
observers that matched. Each method returns its receiver for chaining.

| Hook | TypeScript | Python | Rust | Go |
| --- | --- | --- | --- | --- |
| every function | `iii.onObserverAdded(h)` | `@iii.on_observer_added` | `iii.on_observer_added(h)` | `client.OnObserverAdded(h)` |
| one function, by id | `iii.onObserverAdded(id, h)` | `@iii.on_observer_added(id)` | `iii.on_observer_added_for(id, h)` | `client.OnObserverAddedFor(id, h)` |
| one function, on its ref | `ref.onObserverAdded(h)` | `@ref.on_observer_added` | `function.on_observer_added(h)` | `ref.OnObserverAdded(h)` |
| match | `iii.onObserverMatch(id, m)`, `ref.onObserverMatch(m)`; `m` is `(obs, call) => boolean` or `{ observer, call }` | `iii.on_observer_match(id, predicate)`, `ref.on_observer_match(predicate)`, or `observer=…, call=…` key functions in place of the predicate | `iii.on_observer_match_for(id, m)`, `function.on_observer_match(m)`; `m` is `Match::predicate(f)` or `Match::keys(observer_keys, call_keys)` | `client.OnObserverMatchFor(id, m)`, `ref.OnObserverMatch(m)`; `m` is `iii.MatchFunc(f)` or `iii.MatchKeys(observerKeys, callKeys)` |
| deliver | `iii.onObserverDeliver(id, h)`, `ref.onObserverDeliver(h)` | `iii.on_observer_deliver(id, h)`, `ref.on_observer_deliver(h)` | `iii.on_observer_deliver_for(id, h)`, `function.on_observer_deliver(h)` | `client.OnObserverDeliverFor(id, h)`, `ref.OnObserverDeliver(h)` |

`onObserverRemoved` follows the same pattern as `onObserverAdded` (`on_observer_removed`,
`on_observer_removed_for`, `OnObserverRemovedFor`). Python hook methods work as decorators and as
plain calls. Added, removed and deliver hooks may be sync or async in Node and Python. A match
predicate and key functions are sync, since they run on every call.

Pass `null` (`None` in Python, `nil` in Go) to remove a hook. Rust removes hooks with `clear_*`
methods: `clear_observer_added()`, `clear_observer_removed()`, `clear_observer_match()` and
`clear_observer_deliver()` on the `FunctionRef`; `clear_observer_added_for(id)`,
`clear_observer_removed_for(id)`, `clear_observer_match_for(id)` and
`clear_observer_deliver_for(id)` on the client; and `clear_observer_added()` and
`clear_observer_removed()` on the client for the worker-wide hooks.

A cron source: one job per distinct expression, and an `onObserverMatch` predicate gives each tick
to that expression's observers.

```typescript
import { ObserveRejection } from "iii-sdk";

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
  .onObserverMatch((obs, call) => obs.args.expression === call.input.expression);
```

```python
from iii import ObserveRejection

# by id, before registering, so the hooks see every observer
@iii.on_observer_added("cron::on-tick")
async def start(obs):  # raise ObserveRejection(code, message) to reject
    expression = obs.args["expression"]
    if not is_valid_cron(expression):
        raise ObserveRejection("INVALID_CRON_EXPRESSION", expression)
    scheduler.retain(expression, fire)  # your scheduler; fire() calls cron::on-tick

@iii.on_observer_removed("cron::on-tick")
def stop(obs):
    scheduler.release(obs.args["expression"])

iii.on_observer_match("cron::on-tick",
                      lambda obs, call: obs.args["expression"] == call.input["expression"])

cron_args = {"type": "object", "required": ["expression"],
             "properties": {"expression": {"type": "string"}}}
iii.register_function("cron::on-tick", lambda tick: tick, args_format=cron_args, private=True)

async def fire(expression, scheduled_time):
    await iii.call_async({"function_id": "cron::on-tick", "payload": {
        "expression": expression, "scheduled_time": scheduled_time, "actual_time": now()}})
```

```rust
use iii_sdk::{Error, Match, ObserveRejection, Observer, RegisterFunction};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
struct CronArgs { expression: String }

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
struct CronTick { expression: String, scheduled_time: String, actual_time: String }

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
iii.on_observer_match_for(
    "cron::on-tick",
    Match::predicate(|obs, call| obs.args_value()["expression"] == call.input["expression"]),
);

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
client.OnObserverMatchFor("cron::on-tick", iii.MatchFunc(func(obs *iii.Observer, call *iii.Call) bool {
	var tick struct{ Expression string `json:"expression"` }
	_ = json.Unmarshal(call.Input, &tick)
	return expression(obs.Args) == tick.Expression
}))

_, err := client.RegisterFunction("cron::on-tick", iii.Identity, // returns the tick it is called with
	iii.FunctionOptions{ArgsFormat: cronArgs, Private: true})
// each fire: client.Call(ctx, iii.CallRequest{FunctionID: "cron::on-tick", Payload: tick})
```

The other shapes, in TypeScript (the guide has every language): a database source matches by keys,
an HTTP source delivers each request itself, and a queue source does both.

```typescript
// database::on-row-changed: keys, one lookup per key whatever the number of observers
rowChanged.onObserverMatch({
  observer: ({ args }) => {
    const { db, table, ops } = args as { db: string; table?: string; ops?: string[] };
    return (ops ?? ["*"]).map((op) => `${db}|${table ?? "*"}|${op}`);
  },
  call: ({ input }) =>
    [input.table, "*"].flatMap((t) => [input.op, "*"].map((op) => `${input.db}|${t}|${op}`)),
});

// http::on-request: each request goes to one route, and its target's result is the response
onRequest.onObserverDeliver(async (call) => {
  const route = routes.match(call.input.method, call.input.path);
  if (!route) return { status_code: 404 };
  if (!route.obs.matches(call)) return { status_code: 422 };
  return iii.call({ observer: route.obs, payload: { ...call.input, path_params: route.params } });
});

// queue::on-message: a message matches its subscriber by key; the target's result acks it
onMessage
  .onObserverMatch({
    observer: (obs) => obs.id,
    call: (call) => (call.metadata?.subscriber as string | undefined) ?? [],
  })
  .onObserverDeliver(async (call, [obs]) => {
    if (!obs) throw new ObserveRejection("OBSERVER_NOT_ACTIVE", "subscriber removed"); // retried
    if (!obs.matches(call)) return null; // a filter miss acks the message
    return iii.call({ observer: obs, payload: call.input });
  });
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
  a hook set by id before registering sees every observer. In Node and the browser, a hook chained
  on the returned ref does too. In Python, Rust and Go, set hooks by id before registering. A hook
  set once observers are active runs for each of them at once, and the SDK logs a warning.
- **Matching.** iii delivers a call to the observers `onObserverMatch` matched, and applies each
  one's `on`, filters, policy, and `deliver`. A predicate runs once per active observer per call,
  before filters; one that throws counts as no match, logged once per function. Keys are computed
  once per observer, when it is accepted, and once per call; a call matches the observers filed
  under any of its keys, each once, and an observer whose key function throws is rejected with
  `OBSERVE_REJECTED`. Keys express wildcards by listing them: an observer of every table is filed
  under `"primary|*|insert"`, and a change in `orders` has the keys `"primary|orders|insert"`,
  `"primary|orders|*"`, `"primary|*|insert"`, and `"primary|*|*"`. Keys suit sources whose
  observers each want a slice of many calls: row changes by table, frames by session, messages by
  subscriber.
- **Delivering yourself.** `onObserverDeliver(call, observers)` receives the matched observers (a
  snapshot taken when the call started) and runs inside the function's invocation, so calls it
  makes have that invocation as their caller. iii delivers nothing for that function. The hook
  checks an observer's `on` and filters with `obs.matches(call)` and calls
  `iii.call({ observer: obs, payload })` (Python `call_async({"observer": obs, ...})`, Rust
  `CallRequest::observer(&obs, payload)`, Go `iii.CallRequest{Observer: obs, ...}`), which resolves
  to the target's result. Only the worker that hosts the function may call through its observers
  (`NOT_FUNCTION_OWNER`).
- **Match or deliver.** Choose observers in `onObserverMatch` when an observer's args and the call
  settle it (a cron expression, a table, a subscriber id). Choose them in `onObserverDeliver` when
  choosing compares observers with each other or produces part of the payload: `http::on-request`
  picks the most specific route pattern that fits the path and puts its path parameters in the
  target's request, so it sets no match hook and its deliver hook receives every active observer.
- **What the caller gets.** The handler's return value is what observers observe and what the
  caller gets back. With `onObserverDeliver`, the caller gets what the hook returns or throws: an
  HTTP source returns the route target's response.
- **`isObserved(call?)`** says whether a call would reach anyone, so a source can skip building
  events nobody wants. Without a call it is true when the function has any active observer: one
  atomic load. With a call, on a function matched by keys, it is true when an observer is filed
  under one of the call's keys. Python `is_observed(call=None)`, Rust `is_observed()` /
  `is_observed_for(&call)`, Go `IsObserved()` / `IsObservedFor(call)`. `observers()` lists the
  active ones.
- **A call for one observer.** To send one observer something of its own (its initial state, an
  event it missed), a source calls its own function with `metadata: { for_observer: obs.id }`, and
  its match hook gives that call to that observer alone: a predicate compares
  `call.metadata.for_observer` with `obs.id`; key functions also file each observer under
  `for_observer:<id>` and give such a call that one key. iii delivers it like every call, with the
  observer's `deliver`, `capture`, filters, `on`, and policy, so its target receives every event of
  the function in one shape. Under RBAC middleware, a host's call of a function it does not host
  privately goes through the middleware, which receives the metadata as its own: make the function
  private, or have the middleware forward the call's metadata. A call through an observer belongs
  in a deliver hook.
- **`obs.accepted()`** resolves once the SDK has acknowledged the observer. A call made afterwards
  reaches an active observer, so a source sends initial state as a call for that observer. The
  observer becomes active after `onObserverAdded` returns, so the hook starts a task that waits and
  then makes the call with `CallAction.Void()`. `accepted()` fails for an observer rejected first
  (a match key function that throws, `obs.reject`), and the task then sends nothing: in Node and
  the browser, end the chain with `.catch`. Python awaits `accepted_async()` in a task
  (`asyncio.create_task`), since `accepted()` blocks the calling thread; Rust awaits
  `obs.accepted()` in a spawned task; Go runs `obs.Accepted(ctx)` in a goroutine with a context of
  its own, since the hook's `ctx` makes it return an error at once.

```typescript
// a new observer gets the state of its session; every other change goes to the session's observers
iii.onObserverAdded("tmux::on-changed", (obs) => {
  void obs
    .accepted()
    .then(() =>
      iii.call({
        function_id: "tmux::on-changed",
        payload: stateFor(obs.args),
        metadata: { for_observer: obs.id },
        action: CallAction.Void(),
      }),
    )
    .catch(() => {
      // a rejected observer gets nothing
    });
});
iii.onObserverMatch("tmux::on-changed", (obs, call) =>
  call.metadata?.for_observer
    ? call.metadata.for_observer === obs.id
    : obs.args.session === call.input.session,
);
```

- **Registration options.** `args_format` (JSON Schema for observers' args; `INVALID_ARGS` when
  they fail it). `private: true`: only this worker may call the function, others may observe it,
  and the worker's own calls pass input and result by reference. `observable: false`: nobody may
  observe it (`NOT_OBSERVABLE`), for functions whose payloads carry credentials. `standby: true`:
  when another instance already hosts the id, retry the registration every second (±20 % jitter)
  and take over its observers when it leaves. A
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
| Registering a function | `INVALID_ARGS_FORMAT`, `FUNCTION_NAMESPACE_CONFLICT` (another connection hosts the id; see `standby`), `UNSUPPORTED` (a function with `args_format`, `private`, `observable: false`, `onObserverMatch` or `onObserverDeliver` on an engine without `observe.v2`: the SDK logs it once and registers the function when it connects to an engine that has it) |
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
  `onObserverRemoved`, `onObserverMatch`, `onObserverDeliver`, and `private`.
- Use it when code written for 0.25 needs to move to 0.26.

## Boundaries

- For registering functions, calling them (`call`, void, enqueue), workers, and the registry, use
  `iii-core-primitives`.
- For the `observers:` engine config section and RBAC hooks, use `iii-engine-config`.
- For package exports and language syntax, use `iii-sdk-reference`; for error handling in general,
  use `iii-error-handling`.
- A capability worker's source function, its `args`, and what its target receives are documented
  by that worker. Use the ids in this skill and the worker's README; do not invent others.
