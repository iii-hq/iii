# iii-sdk

Rust SDK for the [iii engine](https://github.com/iii-hq/iii).

[![crates.io](https://img.shields.io/crates/v/iii-sdk)](https://crates.io/crates/iii-sdk)
[![docs.rs](https://img.shields.io/docsrs/iii-sdk)](https://docs.rs/iii-sdk)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](../../../LICENSE)

## Install

Add to your `Cargo.toml`:

```toml
[dependencies]
iii-sdk = "0.26"
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

### OpenTelemetry

The SDK exposes OpenTelemetry types (`Context`, `KeyValue`, tracers, span
processors) through the `iii-helpers` crate, which re-exports the exact
`opentelemetry` version it was built with. Use that path instead of adding
`opentelemetry` to your own `Cargo.toml`, so the versions always match:

```toml
iii-helpers = "0.26"
```

```rust
use iii_helpers::observability::opentelemetry::{KeyValue, global};
```

If you must depend on `opentelemetry` directly, match this table. Each
OpenTelemetry minor is a distinct set of types, so every row is a breaking
change for direct dependents and ships as a minor bump of `iii-sdk`:

| iii-sdk / iii-helpers | opentelemetry, opentelemetry_sdk | tracing-opentelemetry |
|-----------------------|----------------------------------|-----------------------|
| next release          | 0.32                             | 0.33                  |
| 0.23, 0.24            | 0.31                             | 0.32                  |

## Hello World

```rust
use iii_sdk::{register_worker, CallRequest, Error, InitOptions, RegisterFunction, Source};
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii = register_worker("ws://localhost:49134", InitOptions::default());

    iii.register_function(
        "hello::greet",
        RegisterFunction::new_async(|input: Value| async move {
            let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("world");
            Ok::<_, Error>(json!({ "message": format!("Hello, {name}!") }))
        }),
    );

    // Run hello::greet for every POST /greet (the http worker hosts http::on-request)
    iii.observe(
        Source::function("http::on-request").args(json!({ "method": "POST", "path": "/greet" })),
        "hello::greet",
    );

    let result: Value = iii
        .call(CallRequest::new("hello::greet", json!({ "name": "world" })))
        .await?;

    println!("result: {result}");
    Ok(())
}
```

## API

| Operation                  | Signature                                                                    | Description                                                     |
| -------------------------- | ---------------------------------------------------------------------------- | --------------------------------------------------------------- |
| Initialize                 | `register_worker(address, options)`                                          | Create an SDK instance and auto-connect                         |
| Register function          | `iii.register_function(id, RegisterFunction::new_async(f))`                  | Register a function that can be called by id                    |
| Call (await)               | `iii.call(CallRequest::new(id, payload)).await?`                             | Call a function and wait for its result                         |
| Call (fire-and-forget)     | `iii.call(CallRequest::new(id, payload).action(CallAction::Void)).await?`    | Fire-and-forget call                                            |
| Call (enqueue)             | `iii.call(CallRequest { action: Some(CallAction::Enqueue { queue }), .. })`  | Route the call through a named queue                            |
| Observe a function         | `iii.observe("orders::create", target)`                                      | Run `target` after every successful call of a function          |
| Observe with args, filters | `iii.observe(Source::function(id).args(..).filters(..), target)`             | Args for the function's worker, filters for which calls         |
| Observer hooks             | `iii.on_observer_added_for(id, h)`, `..._removed_for`, `..._match_for`, `..._deliver_for` | What the worker hosting a function does when it is observed |
| Call through an observer   | `iii.call(CallRequest::observer(&obs, payload)).await?`                      | Run an observer's target from a deliver hook                    |

`trigger`, `TriggerRequest` and `TriggerAction` are `call`, `CallRequest` and
`CallAction` under their 0.25 names.

`register_worker()` spawns a background task that handles WebSocket communication, automatic
reconnection, and OpenTelemetry instrumentation.

### Registering Functions

```rust
use iii_sdk::{Error, RegisterFunction};
use serde_json::{json, Value};

iii.register_function(
    "orders::create",
    RegisterFunction::new_async(|input: Value| async move {
        let item = input["body"]["item"].as_str().unwrap_or("");
        Ok::<_, Error>(json!({ "status_code": 201, "body": { "id": "123", "item": item } }))
    }),
);
```

### Calling Functions

```rust
use iii_sdk::{CallAction, CallRequest};
use serde_json::json;

// Synchronous: waits for the result
let result = iii
    .call(CallRequest::new("orders::create", json!({ "body": { "item": "widget" } })))
    .await?;

// Fire-and-forget
iii.call(CallRequest::new("analytics::track", json!({ "event": "page_view" })).action(CallAction::Void))
    .await?;

// Async via named queue
iii.call(CallRequest {
    function_id: "orders::process".to_string(),
    payload: json!({ "order_id": "456" }),
    action: Some(CallAction::Enqueue { queue: "payments".to_string() }),
    timeout_ms: None,
})
.await?;
```

A worker calling one of its own functions runs it in-process (no frames on
the wire unless an observer's filters match);
`CallRequest::dispatch(Dispatch::Remote)` forces the engine path.

### Observing Functions

When a function runs, its observers run. `observe(source, target)` calls
`target` (a registered function) after each call of `source` that its filters
match; what it observes is the function's return value. `args` are for the
worker that hosts the function (a route, a cron expression), `filters` pick
the calls:

```rust
use iii_sdk::{Capture, On, ObserverOptions, Source};

// every successful call of orders::create
iii.observe("orders::create", "search::reindex-order");

let alerts = iii.observe_with(
    Source::function("payments::charge")
        .on(On::Error)
        .filters(json!({ "input.amount": { "$gte": 500 } }))
        .capture([Capture::Input, Capture::Error, Capture::Caller]),
    "alerts::payment-failed",
    ObserverOptions::default().max_in_flight(4),
);
alerts.ready().await?;
```

An observer's id is `ObserverOptions::id`, or a fresh UUID. Its handle holds
the id until `dispose()` is called or the engine disposes the observer, and the
id is free again at once: a queue consumer observes again under its stable id
right after it disposes the observer before. While a handle of this client
holds the id, in any state, another `observe_with` under it sends nothing and
leaves that handle alone: the handle it returns is in state `error` with the
reason `OBSERVER_ID_CONFLICT` (`observer id already in use: <id>`), `ready()`
fails with that code, and its `dispose()` sends nothing. Each handle of an id
hears the engine's answers to its own messages only, so a late `disposed` for
the earlier observer never disposes the new one.

```rust
let messages = || Source::function("queue::on-message").args(json!({ "topic": "orders" }));
let consumer = ObserverOptions::default().id("orders-consumer");
let first = iii.observe_with(messages(), "orders::consume", consumer.clone());
first.dispose().await?;
let again = iii.observe_with(messages(), "orders::consume", consumer);
again.ready().await?;
```

The engine's own events are functions too: `iii_sdk::engine::EngineSources`
names them (`engine::on-functions-changed`, `observability::on-log`, ...).

### Hosting Observable Functions

The worker that hosts a function decides what observing it means with four
hooks, each overriding what iii does by default:

| Hook                  | The question it answers                                                         | What iii does by default                                    |
| --------------------- | ------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `on_observer_added`   | What do I set up for this observer? Return an `ObserveRejection` to reject it.  | accepts the observer                                        |
| `on_observer_removed` | What do I tear down?                                                            | forgets the observer                                        |
| `on_observer_match`   | Does this observer want this call?                                              | every observer wants every call                             |
| `on_observer_deliver` | I deliver this call myself: which observers, and what does the caller get back? | delivers to every matching observer whose filters match it  |

Added and removed hooks run for one function (`..._for(id, hook)`, or on the
`FunctionRef`) and for every function the worker hosts (`on_observer_added(hook)`):
the worker's then the function's when an observer is added, the reverse when
it is removed. Match and deliver hooks are set per function. Async closures
and your own types implement the hook traits. Set hooks by id before
registering the function, so they see every observer.

Filters are iii's, and run where the call's parts are. The engine evaluates
the `on` and filters of every call it routes: it holds the input, the result
or error, the caller and the metadata. The SDK evaluates those of the calls a
worker makes to its own functions, which run in-process and never reach the
engine. For a function without a match hook, it matches a call through an
index of the observers' equality conditions (an `$eq` or `$in` of a scalar in
the top-level `$and`): one lookup per indexed path, plus one evaluation per
observer the index does not rule out. A set of 16 observers or fewer is
scanned, every filter evaluated, and so is one in which most observers accept
the call and have no such condition. The observers a match hook matched are
each evaluated.

The match and deliver hooks are the worker's code, and the SDK runs them for
the calls of their function, routed or local: the deliver hook for every
call, the match hook for every call while the function has an active
observer on the worker or a deliver hook.

An event source is a function whose handler returns the event it is called
with, `private` so only its worker calls it:

```rust
use iii_sdk::{Call, CallRequest, Error, Match, ObserveRejection, Observer, RegisterFunction};

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct CronArgs { expression: String }

iii.on_observer_added_for("cron::on-tick", |obs: Observer| async move {
    let CronArgs { expression } = obs.args()?; // validated by the engine
    if expression.split_whitespace().count() != 7 {
        return Err(ObserveRejection::new("INVALID_CRON_EXPRESSION", expression));
    }
    Ok(()) // start the expression's job
});
// an observer of expression X wants the ticks of expression X; by keys, iii
// keeps an index, however many observers there are
iii.on_observer_match_for(
    "cron::on-tick",
    Match::keys(
        |obs| obs.args_value()["expression"].as_str().into(),
        |call| call.input["expression"].as_str().into(),
    ),
);
let on_tick = iii.register_function(
    "cron::on-tick",
    RegisterFunction::new(|tick: serde_json::Value| Ok::<_, Error>(tick))
        .args_format::<CronArgs>()
        .private(),
);

// a tick: skipped when nobody observes its expression
let call = Call { input: json!({ "expression": "0 0 9 * * * *" }), ..Call::default() };
if on_tick.is_observed_for(&call) {
    iii.call(CallRequest::new("cron::on-tick", call.input)).await?;
}
```

A deliver hook takes delivery over: it gets the observers that matched, calls
their targets through them, applies their filters with `obs.matches(&call)`,
and returns what the caller gets back:

```rust
use iii_sdk::{Call, CallRequest, Observer};

iii.on_observer_deliver_for("http::on-request", {
    let iii = iii.clone();
    move |call: Call, observers: Vec<Observer>| {
        let iii = iii.clone();
        async move {
            let Some(route) = observers.first() else {
                return Ok(json!({ "status_code": 404 }));
            };
            if !route.matches(&call) {
                return Ok(json!({ "status_code": 422 }));
            }
            iii.call(CallRequest::observer(route, call.input.clone())).await
        }
    }
});
```

To send one observer something of its own, such as its initial state, call
the function with metadata `{ "for_observer": <observer id> }` once
`obs.accepted()` resolves, and let the match hook give that call to that
observer alone. iii delivers it like every call, so the target receives it in
the shape it receives every call of the function:

```rust
use iii_sdk::{CallAction, CallRequest, Match, Observer};

iii.on_observer_added_for("tmux::on-changed", {
    let iii = iii.clone();
    move |obs: Observer| {
        let iii = iii.clone();
        async move {
            tokio::spawn(async move {
                if obs.accepted().await.is_ok() {
                    let state = CallRequest::new("tmux::on-changed", state_for(obs.args_value()))
                        .metadata(json!({ "for_observer": obs.id() }));
                    let _ = iii.call(state.action(CallAction::Void)).await;
                }
            });
            Ok(())
        }
    }
});
iii.on_observer_match_for(
    "tmux::on-changed",
    Match::predicate(|obs, call| {
        match call.metadata.as_ref().and_then(|m| m["for_observer"].as_str()) {
            Some(id) => id == obs.id(),
            None => obs.args_value()["session"] == call.input["session"],
        }
    }),
);
```

With keys, file each observer under `for_observer:<id>` as well, and give such
a call that one key.

Functions with `args_format`, `private`, `observable(false)`, a match hook or
a deliver hook (`on_observer_match`, `on_observer_deliver`) need an engine
that advertises `observe.v2`; against an older one they stay unregistered and
the SDK logs `UNSUPPORTED` once.

### Stream Operations

> **Deprecated.** The `stream::*` functions, `IStream`, `iii_sdk::helpers::create_stream` and the legacy
> Streams-only models in `iii_helpers::stream` (`Stream*Input`, `StreamChangeEvent`, ...) are deprecated
> (iii-stream) and will be removed in an upcoming release. Behavior is unchanged for now. Migration guide:
> https://iii.dev/docs/upgrading/migrate-from-streams
>
> `UpdateOp`, `MergePath`, `UpdateOpError` and `StreamSetResult` / `StreamUpdateResult` / `StreamDeleteResult`
> are shared with `state::*` and stay supported.

```rust
use iii_helpers::stream::UpdateOp;
use iii_sdk::{register_worker, CallRequest, InitOptions};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii = register_worker("ws://localhost:49134", InitOptions::default());

    // Set a stream item
    iii.call(CallRequest::new(
        "stream::set",
        json!({
            "stream_name": "users",
            "group_id": "active",
            "item_id": "user-1",
            "data": { "status": "online" },
        }),
    ))
    .await?;

    // Atomic update ops
    let ops = vec![
        UpdateOp::increment("total", 100),
        UpdateOp::set("status", json!("processing")),
    ];

    iii.call(CallRequest::new(
        "stream::update",
        json!({
            "stream_name": "orders",
            "group_id": "user-123",
            "item_id": "order-456",
            "ops": ops,
        }),
    ))
    .await?;

    Ok(())
}
```

### Logger

```rust
use iii_helpers::observability::Logger;

let logger = Logger::new(Some("my-function".to_string()));
logger.info("Processing started", None);
```

The `Logger` struct emits OTel `LogRecord`s, falling back to the `tracing` crate when OTel is not
initialized.

## Resources

- [Documentation](https://iii.dev/docs)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)
