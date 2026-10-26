---
name: iii-observe
description: >-
  Use when observing iii functions with iii.observe, writing filters, reacting to engine events,
  writing observable functions with owner delivery (onObserverAdded, obs.emit, obs.call), tuning
  local dispatch, or migrating from the deprecated registerTrigger / registerTriggerType across
  TypeScript, browser, Python, Rust, and Go.
---

# Observe

Since iii 0.25 every function is observable. `iii.observe(source, target, options?)` connects a
source function to a target function: each event of the source that passes the observer's filters
invokes the target. It replaces `registerTrigger`, and an `observe` descriptor on
`registerFunction` replaces `registerTriggerType`. Both old APIs still work in 0.25 (see
[Migrating from triggers](#migrating-from-triggers)).

| Part | Fields | Notes |
| --- | --- | --- |
| `source` (left side) | `function_id`, `namespace?`, `args?`, `filters?`, `on?`, `capture?`, `deliver?` | `args` only for owner-delivered functions. `on` (auto only): `success` (default), `error`, `complete`. `capture` (auto and `obs.emit`): default `['input', 'output', 'caller']`, `[]` allowed. `deliver`: `envelope` (default), `input`, `output` |
| `target` (right side) | a registered function id (or `{ function_id, namespace }`) | No inline handler. The observer stays `pending` until the target is registered |
| `options` | `id?`, `max_fires?`, `expires_in_ms?`, `max_in_flight?`, `metadata?` | `id` defaults to a UUID kept across replays. `metadata` is the sidecar of every activation |

The source namespace resolves like `trigger()`: the worker's namespace, except `engine::*` and
`configuration::*`, which resolve in `default`.

## Observe a function

Register the target first, then observe. This alerts on failed charges of 500 or more:

```typescript
import { registerWorker, type Observed } from "iii-sdk";

const iii = registerWorker(process.env.III_URL ?? "ws://localhost:49134");

iii.registerFunction("alerts::payment-failed", async (event: Observed<{ amount: number }>) => {
  console.log(event.function_id, event.error?.code, event.input?.amount, event.caller?.worker_name);
  return null; // ignored for auto delivery
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
```

The browser SDK (`iii-browser-sdk`) has the same `observe` API as Node.

## What the target receives

- **Envelope** (default `deliver`): `{ observer_id, activation_id, function_id, namespace,
  invocation_id, status, input?, output?, error?, caller?, metadata?, started_at_ms, duration_ms }`.
  Only captured fields are present. `deliver: 'input'` or `'output'` sends that value alone.
- The observer's `options.metadata` arrives as the handler's metadata argument (Node and browser
  second argument, Python `metadata` parameter, the `Option<Value>` of a Rust
  `Fn(Value, Option<Value>)` handler, Go `iii.MetadataFromContext(ctx)`).
- Activations are fire-and-forget and at most once; the target's return value is discarded, except
  for `obs.call` (below). Drops are counted, never silent.

## Handle and states

`pending` (no owner, no ack, or no target yet) → `active` or `error`. `suspended` while the
target is away, or while the observing worker is disconnected (30 s grace, then `disposed`).
`disposed` after `dispose()`, `max_fires`, or expiry. `observe()` returns synchronously, works before the connection is up, and is
replayed with the same id on reconnect. `onStateChange(listener)` (Python `on_state_change`, Go
`OnStateChange`, Rust `on_state_change`) reports every transition with a reason code.

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
  coercion; no implicit fan-out over arrays (use `$any`).
- Limits: depth 8, 64 nodes, 256 `$in` items, 16 path segments, 8 KiB as written.
- Paths are checked against the function's `request_format` / `response_format`: an unknown
  property under a closed schema is `INVALID_FILTER_PATH`.

```json
{ "caller.parent.worker_name": { "$ne": "search-indexer" },
  "input.items": { "$any": { "@.sku": { "$prefix": "VIP-" } } } }
```

## Observable functions (owner delivery)

A function decides how its observers are delivered with an `observe` descriptor. Full guide:
https://iii.dev/docs/creating-workers/observable-functions.

| | `auto` (default) | `owner` |
| --- | --- | --- |
| Events | every invocation | what the owner hands over |
| Args | none (`INVALID_ARGS` if sent) | validated by the engine against `args_format` |
| Delivery | engine or host SDK, after the call | `obs.emit(event)` (fire-and-forget) or `obs.call(payload)` (returns the target's result) |

```typescript
import { type Observer, ObserveRejection } from "iii-sdk";

const schedules = new Map<string, Set<Observer>>(); // expression -> observers

const onTick = iii.registerFunction(
  "cron::on-tick",
  async (tick: { expression: string; scheduled_time: string }) => {
    for (const obs of schedules.get(tick.expression) ?? []) obs.emit(tick); // false: filters said no
    return null;
  },
  {
    observe: {
      delivery: "owner",
      args_format: { type: "object", required: ["expression"], properties: { expression: { type: "string" } } },
      legacy_trigger_type: "cron", // optional: old registerTrigger({ type: 'cron' }) consumers keep working
      onObserverAdded(obs) {
        const expression = obs.args.expression as string;
        if (!isValidCron(expression)) throw new ObserveRejection("INVALID_CRON_EXPRESSION", expression); // your validator
        if (!schedules.has(expression)) schedules.set(expression, new Set());
        schedules.get(expression)!.add(obs); // index before the ack
      },
      onObserverRemoved(obs) {
        schedules.get(obs.args.expression as string)?.delete(obs);
      },
    },
  },
);
// your scheduler fires: iii.trigger({ function_id: "cron::on-tick", payload: tick }) (runs in-process)
// onTick.isObserved() is one load: skip building events nobody receives
```

```python
from iii import ObserveHooks, ObserveRejection

schedules: dict[str, set] = {}  # expression -> observers

def on_tick(tick):
    for obs in list(schedules.get(tick["expression"], ())):
        obs.emit(tick)  # False when the observer's filters reject the tick

def added(obs):  # sync or async; raise ObserveRejection(code, message) to refuse
    schedules.setdefault(obs.args["expression"], set()).add(obs)

def removed(obs):
    schedules.get(obs.args["expression"], set()).discard(obs)

iii.register_function("cron::on-tick", on_tick, observe=ObserveHooks(
    delivery="owner",
    args_format={"type": "object", "required": ["expression"],
                 "properties": {"expression": {"type": "string"}}},
    on_observer_added=added,
    on_observer_removed=removed,
))
```

```rust
use iii_sdk::{Error, Observe, ObserveHandler, ObserveRejection, Observer, RegisterFunction};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::{Arc, Mutex}};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CronArgs { expression: String }

#[derive(Serialize, Deserialize, JsonSchema)]
struct CronTick { expression: String, scheduled_time: String }

#[derive(Clone, Default)]
struct Schedules(Arc<Mutex<HashMap<String, Vec<Observer>>>>); // expression -> observers

#[async_trait::async_trait] // ObserveHandler needs the async-trait crate
impl ObserveHandler for Schedules {
    async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
        let CronArgs { expression } = obs.args()?; // validated by the engine
        self.0.lock().unwrap().entry(expression).or_default().push(obs.clone());
        Ok(())
    }
    async fn on_observer_removed(&self, obs: &Observer) {
        for list in self.0.lock().unwrap().values_mut() { list.retain(|o| o.id() != obs.id()); }
    }
}

let schedules = Schedules::default();
iii.register_function(
    "cron::on-tick",
    RegisterFunction::new({
        let schedules = schedules.clone();
        move |tick: CronTick| {
            let observers = schedules.0.lock().unwrap().get(&tick.expression).cloned();
            for obs in observers.unwrap_or_default() { obs.emit(&tick)?; } // Ok(false): filters said no
            Ok::<_, Error>(())
        }
    })
    .observe(Observe::owner().args_format::<CronArgs>().handler(schedules)),
);
```

```go
// onTick is the function's handler; index is your expression -> observers map.
client.RegisterFunction("cron::on-tick", onTick, iii.RegisterFunctionOptions{Observe: &iii.ObserveHooks{
	Delivery:   iii.DeliveryOwner,
	ArgsFormat: json.RawMessage(`{"type":"object","required":["expression"],"properties":{"expression":{"type":"string"}}}`),
	OnObserverAdded: func(ctx context.Context, obs *iii.Observer) error {
		expr, _ := obs.Args["expression"].(string)
		return index.Add(expr, obs) // or iii.ObserveRejection("INVALID_CRON_EXPRESSION", "...")
	},
	OnObserverRemoved: func(ctx context.Context, obs *iii.Observer) { index.Remove(obs) },
}})
// inside onTick: ok, err := obs.Emit(ctx, tick); result, err := obs.Call(ctx, payload)
```

Rules:

- `onObserverAdded` runs once per observer, before the ack; the observer turns `active` only after
  it returns. Throwing rejects it (`ObserveRejection` sets the code, anything else is
  `OBSERVE_REJECTED`, 30 s is `OWNER_TIMEOUT`). `onObserverRemoved` cannot fail.
- The owner routes by `obs.args` and never edits filters: the SDK checks the observer's filters in
  `emit` and `call`. `obs.matches(doc)` checks them up front.
- `obs.emit` returns `false` when the filters reject the event or the observer is inactive.
  `obs.call` rejects with `FILTERS_NOT_MATCHED` or `OBSERVER_NOT_ACTIVE`; both throw
  `NOT_OWNER_DELIVERED` on an auto function. `obs.reject(code, message)` is a late rejection.
- `args_format` with `delivery: 'auto'` fails at registration (`INVALID_OBSERVE_DESCRIPTOR`).
  `allow: false` refuses every observer (`NOT_OBSERVABLE`). `obs.legacy` is set for observers the
  engine created from an old `registerTrigger` (its `config` replaces the args).

## Local dispatch

A worker calling a function it registered itself runs it in-process once the engine confirmed it
(`functionregistered`): no frames when nobody observes it; matching calls are reported in batches.
The handler gets a copy of the payload, there is no 16 MiB limit on that path, and a Rust sync
handler runs inline on the caller's task. Opt out per call (`dispatch: 'remote'`, Rust
`.dispatch(Dispatch::Remote)`, Go `Dispatch: iii.DispatchRemote`), per function
(`localDispatch: false`, Python `local_dispatch=False`, Rust `.local_dispatch(false)`, Go
`LocalDispatch: &off`), or engine-wide (`observers.local_dispatch: false`). Enqueue calls always
go through the engine.

## Caller and loops

- `caller` is `{ worker_id, worker_name, function_id?, invocation_id?, parent? }`. `worker_*` is
  stamped by the engine; `parent` is the caller of the handler the call came from, one level only.
  Filter on it to stop echo loops: `{ "caller.parent.worker_name": { "$ne": "indexer" } }`.
- Read the current invocation with `iii.currentInvocation()` (Python `iii.current_invocation()`,
  Rust `iii_sdk::current_invocation()`, Go `iii.InvocationFromContext(ctx)`). In Go pass the
  handler's `ctx` to `Trigger`; in Rust code you `tokio::spawn` loses it unless wrapped in
  `iii_sdk::scope(ctx, fut)`; in the browser it ends at the first `await` (use
  `iii.withInvocation(inv, fn)`).
- Every activation is one observe hop deeper. Beyond `observers.max_observe_depth` (default 8) it
  is dropped as `LOOP_GUARD`. A target equal to the observed function is `SELF_TARGET`.

## Engine-hosted event sources

Auto functions in the `default` namespace whose events the engine emits. They have no output, so
observe them with `deliver: 'input'`. Calling one directly is `FORBIDDEN`. The old trigger types
keep firing.

| Function | Replaces trigger type | `input` |
| --- | --- | --- |
| `engine::on-functions-changed` | `engine::functions-available` | `{ event: "functions_changed", functions: [...] }`, bursts coalesced (~100 ms) |
| `engine::on-workers-changed` | `engine::workers-available` | `{ event, worker_id }`, `event` one of `worker_connected`, `worker_disconnected`, `worker_metadata_updated` |
| `configuration::on-updated` | `configuration` | `{ type, event_type, id, name, ..., old_value, new_value }` |
| `observability::on-log` | `log` | one log record |
| `observability::on-span` | `trace` | `{ trace_ids: [...] }` per window (~300 ms) |

```typescript
iii.observe({ function_id: "engine::on-workers-changed", deliver: "input" }, "ops::on-worker");
iii.observe(
  { function_id: "configuration::on-updated", deliver: "input",
    filters: { "input.id": { $eq: "orders" }, "input.event_type": { $eq: "configuration:updated" } } },
  "orders::reload",
);
// observability::* does not resolve to default automatically: name it from a namespaced worker
iii.observe(
  { function_id: "observability::on-log", namespace: "default", deliver: "input",
    filters: { "input.severity_text": { $eq: "ERROR" } } },
  "alerts::log",
);
```

## Migrating from triggers

`registerTrigger` and `registerTriggerType` (every SDK and spelling) are deprecated in 0.25. They
keep working, log one warning per process, and will be removed in a future release. Guide:
https://iii.dev/docs/upgrading/from-0-24-x.

| 0.24 | 0.25 |
| --- | --- |
| `registerTrigger({ type, function_id, config, metadata })` | `observe({ function_id: <source>, args: config, filters }, function_id, { metadata })` |
| `trigger.unregister()` | `handle.dispose()` |
| `registerTriggerType({ id }, handler)` | `registerFunction('<worker>::on-<event>', handler, { observe: { delivery: 'owner', ... } })` |
| `TriggerHandler.registerTrigger(config)` / `unregisterTrigger` | `onObserverAdded(obs)` (`obs.args`) / `onObserverRemoved(obs)` |
| provider fan-out with `iii.trigger(binding.function_id)` | `obs.emit(event)` or `obs.call(payload)` |
| `trigger_request_format` / `call_request_format` | `observe.args_format` / the function's `request_format` |
| `condition_function_id` | `filters` for declarative conditions; a condition that needs code stays on `registerTrigger` or moves into the target |

**The built-in providers have not migrated in 0.25.** The `http`, `cron`, `queue`
(`durable:subscriber`), `state`, `stream`, `pubsub` (`subscribe`) and `database` workers still
register trigger types and do not host source functions (`http::on-request`, `cron::on-tick`, ...)
yet. Keep `registerTrigger` for them; the warning is informational. Use `observe` for regular
functions, the engine-hosted sources above, and providers that ship source functions. A provider
that migrates declares `legacy_trigger_type` so existing `registerTrigger` consumers keep working.

## Error codes

| When | Codes |
| --- | --- |
| Creating an observer (`ready()` rejects) | `UNSUPPORTED` (engine older than 0.25, or an unknown `on`/`capture`/`deliver`), `INVALID_ARGS`, `INVALID_FILTER`, `INVALID_FILTER_PATH`, `NOT_OBSERVABLE`, `FORBIDDEN`, `SELF_TARGET`, `LIMIT_EXCEEDED`, `OBSERVER_ID_CONFLICT`, `OWNER_TIMEOUT`, `OBSERVE_REJECTED`, or the owner's own code |
| `obs.call` | `FILTERS_NOT_MATCHED`, `OBSERVER_NOT_ACTIVE`, `NOT_OWNER_DELIVERED`, `OBSERVER_NOT_FOUND`, `NOT_FUNCTION_OWNER`, `TARGET_MISMATCH`, `LOOP_GUARD`, `OVERFLOW`, `TIMEOUT`, or the target's error |
| State reasons | `OWNER_UNAVAILABLE`, `TARGET_UNAVAILABLE`, `OBSERVER_DISCONNECTED`, `EXPIRED`, `MAX_FIRES`, `UNOBSERVED` |
| Drop reasons (metrics) | `INACTIVE`, `LOOP_GUARD`, `MAX_FIRES`, `OVERFLOW`, `REPORTER_NOT_OWNER`, `CANCELLED`, `INVALID_REPORT`, `BUFFER_FULL`, `TOO_LARGE` |

Inspect observers with `engine::observers::list({ function_id?, namespace?, worker? })` and
`engine::observers::get({ id })`; `engine::observers::dispose({ id })` removes one. Docs:
https://iii.dev/docs/using-iii/observers.

## When to Use

- Use this skill to run a function when another function is called, fails, or completes, to react
  to engine events (functions, workers, configuration, logs, spans), to write or debug filters, or
  to build an event source other workers observe.
- Use it when migrating `registerTrigger` / `registerTriggerType` code to 0.25, or when a
  deprecation warning about them appears.

## Boundaries

- To bind functions to `http`, `cron`, queue, `state`, `stream`, pub/sub, or database events in
  0.25, use `registerTrigger` as taught in `iii-core-primitives`; those providers have not migrated.
- For the `observers:` engine config section and RBAC hooks, use `iii-engine-config`.
- For package exports and language syntax, use `iii-sdk-reference`; for error handling in general,
  use `iii-error-handling`.
- Do not invent source functions (`http::on-request`, `cron::on-tick`, `database::on-row-changed`)
  as available: they exist only once the provider worker ships them.
