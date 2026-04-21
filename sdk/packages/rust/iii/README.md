# iii-sdk

Rust SDK for the [iii engine](https://github.com/iii-hq/iii).

[![crates.io](https://img.shields.io/crates/v/iii-sdk)](https://crates.io/crates/iii-sdk)
[![docs.rs](https://img.shields.io/docsrs/iii-sdk)](https://docs.rs/iii-sdk)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](../../../LICENSE)

## Prerequisites

The SDK connects to a running iii engine over WebSocket. Before running the snippets below:

1. Install the engine: `curl -fsSL https://install.iii.dev/iii/main/install.sh | sh`
2. Start the engine in a separate terminal: `iii --config config.yaml`
   (default WebSocket URL: `ws://localhost:49134`)

See the [iii quickstart](https://iii.dev/docs/quickstart) for scaffolding a full project.

## Install

Add to your `Cargo.toml`:

```toml
[dependencies]
iii-sdk = "0.11"
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

## Hello World

```rust,no_run
use iii_sdk::builtin_triggers::{HttpMethod, HttpTriggerConfig};
use iii_sdk::{IIITrigger, InitOptions, RegisterFunction, TriggerRequest, register_worker};
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii = register_worker("ws://localhost:49134", InitOptions::default());

    iii.register_function(RegisterFunction::new_async("greet", |input: Value| async move {
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("world");
        Ok::<Value, iii_sdk::IIIError>(json!({ "message": format!("Hello, {name}!") }))
    }));

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/greet").method(HttpMethod::Post))
            .for_function("greet"),
    )?;

    let result: Value = iii
        .trigger(TriggerRequest::new("greet", json!({ "name": "world" })))
        .await?;

    println!("result: {result}");
    iii.shutdown();
    Ok(())
}
```

This snippet is kept in sync with [`examples/readme_hello.rs`](./examples/readme_hello.rs)
and verified by CI via `cargo check --example readme_hello`.

`register_worker()` returns `III` directly (not `Result`). It spawns a background
task that handles WebSocket communication, automatic reconnection, and
OpenTelemetry instrumentation.

## API

| Operation                | Signature                                                         | Description                                            |
| ------------------------ | ----------------------------------------------------------------- | ------------------------------------------------------ |
| Initialize               | `register_worker(address, options)`                               | Create an SDK instance and auto-connect                |
| Register function        | `iii.register_function(RegisterFunction::new_async(id, handler))` | Register a function that can be invoked by name        |
| Register trigger         | `iii.register_trigger(IIITrigger::Http(...).for_function(id))?`   | Bind a trigger (HTTP, cron, queue, etc.) to a function |
| Invoke (await)           | `iii.trigger(TriggerRequest::new(id, payload)).await?`            | Invoke a function and wait for the result              |
| Invoke (fire-and-forget) | `iii.trigger(TriggerRequest::new(id, payload).with_action(TriggerAction::Void)).await?` | Fire-and-forget invocation |
| Invoke (enqueue)         | `iii.trigger(TriggerRequest::new(id, payload).with_action(TriggerAction::Enqueue { queue })).await?` | Route through a named queue |

### Registering Functions

Use `RegisterFunction::new_async` for async handlers, `RegisterFunction::new` for sync:

```rust,ignore
use iii_sdk::RegisterFunction;
use serde_json::{Value, json};

iii.register_function(RegisterFunction::new_async("orders.create", |input: Value| async move {
    let item = input["body"]["item"].as_str().unwrap_or("");
    Ok::<Value, iii_sdk::IIIError>(json!({
        "status_code": 201,
        "body": { "id": "123", "item": item }
    }))
}));
```

### Registering Triggers

**Recommended — typed builder** (self-documenting, compile-time checked):

```rust,ignore
use iii_sdk::{IIITrigger, builtin_triggers::{HttpMethod, HttpTriggerConfig}};

iii.register_trigger(
    IIITrigger::Http(HttpTriggerConfig::new("/orders").method(HttpMethod::Post))
        .for_function("orders.create"),
)?;
```

**Advanced — raw `RegisterTriggerInput`** (for custom trigger types or dynamic
configs):

```rust,ignore
use iii_sdk::RegisterTriggerInput;
use serde_json::json;

iii.register_trigger(RegisterTriggerInput {
    trigger_type: "http".into(),
    function_id: "orders.create".into(),
    config: json!({ "api_path": "/orders", "http_method": "POST" }),
    metadata: None,
})?;
```

### Invoking Functions

```rust,ignore
use iii_sdk::{TriggerAction, TriggerRequest};
use serde_json::json;

// Synchronous — waits for the result
let result = iii
    .trigger(TriggerRequest::new("orders.create", json!({ "body": { "item": "widget" } })))
    .await?;

// Fire-and-forget
iii.trigger(
    TriggerRequest::new("analytics.track", json!({ "event": "page_view" }))
        .with_action(TriggerAction::Void),
)
.await?;

// Async via named queue
iii.trigger(
    TriggerRequest::new("orders.process", json!({ "order_id": "456" }))
        .with_action(TriggerAction::Enqueue { queue: "payments".into() }),
)
.await?;
```

### Stream Operations

```rust,ignore
use iii_sdk::{TriggerRequest, UpdateBuilder};
use serde_json::json;

iii.trigger(TriggerRequest::new(
    "stream::set",
    json!({
        "stream_name": "users",
        "group_id": "active",
        "item_id": "user-1",
        "data": { "status": "online" },
    }),
))
.await?;

let ops = UpdateBuilder::new()
    .increment("total", 100)
    .set("status", json!("processing"))
    .build();

iii.trigger(TriggerRequest::new(
    "stream::update",
    json!({
        "stream_name": "orders",
        "group_id": "user-123",
        "item_id": "order-456",
        "ops": ops,
    }),
))
.await?;
```

### Logger

```rust,ignore
use iii_sdk::Logger;

let logger = Logger::new(Some("my-function".to_string()));
logger.info("Processing started", None);
```

The `Logger` struct emits OTel `LogRecord`s, falling back to the `tracing` crate
when OTel is not initialized.

## Modules

| Import               | What it provides                                            |
| -------------------- | ----------------------------------------------------------- |
| `iii_sdk`            | Core SDK (`III`, `register_worker`, `TriggerRequest`, etc.) |
| `iii_sdk::stream`    | Stream update builder (`UpdateBuilder`)                     |
| `iii_sdk::logger`    | Structured logging (`Logger`)                               |
| `iii_sdk::telemetry` | OpenTelemetry integration                                   |
| `iii_sdk::types`     | Shared types (`UpdateOp`, `Channel`, `ApiRequest`, etc.)    |

## Resources

- [Documentation](https://iii.dev/docs)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)
