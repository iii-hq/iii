# iii-sdk

Rust SDK for the [iii engine](https://github.com/iii-hq/iii).

[![crates.io](https://img.shields.io/crates/v/iii-sdk)](https://crates.io/crates/iii-sdk)
[![docs.rs](https://img.shields.io/docsrs/iii-sdk)](https://docs.rs/iii-sdk)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](../../../LICENSE)

## Install

Add to your `Cargo.toml`:

```toml
[dependencies]
iii-sdk = "0.11"
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

## Hello World

```rust
use iii_sdk::{register_worker, InitOptions, TriggerRequest};
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii = register_worker("ws://localhost:49134", InitOptions::default());

    iii.register_function("hello::greet", |input: Value| async move {
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("world");
        Ok(json!({ "message": format!("Hello, {name}!") }))
    });

    iii.register_trigger("http", "hello::greet", json!({
        "api_path": "/greet",
        "http_method": "POST"
    }))?;

    let result: Value = iii
        .trigger(TriggerRequest {
        function_id: "hello::greet".to_string(),
            payload: json!({ "name": "world" }),
            action: None,
            timeout_ms: None,
        })
        .await?;

    println!("result: {result}");
    Ok(())
}
```

## API

| Operation                | Signature                                                                                    | Description                                            |
| ------------------------ | -------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| Initialize               | `register_worker(address, options)`                                                          | Create an SDK instance and auto-connect                |
| Register function        | `iii.register_function(id, \|input: Value\| ...)`                                            | Register a function that can be invoked by name        |
| Register trigger         | `iii.register_trigger(type, fn_id, config)?`                                                 | Bind a trigger (HTTP, cron, queue, etc.) to a function |
| Invoke (await)           | `iii.trigger(TriggerRequest { ... }).await?`                                                 | Invoke a function and wait for the result              |
| Invoke (fire-and-forget) | `iii.trigger(TriggerRequest { action: Some(TriggerAction::Void), ... }).await?`              | Fire-and-forget invocation                             |
| Invoke (enqueue)         | `iii.trigger(TriggerRequest { action: Some(TriggerAction::Enqueue { queue }), ... }).await?` | Route invocation through a named queue                 |

`register_worker()` spawns a background task that handles WebSocket communication, automatic
reconnection, and OpenTelemetry instrumentation.

### Registering Functions

```rust
use serde_json::{json, Value};

iii.register_function("orders::create", |input: Value| async move {
    let item = input["body"]["item"].as_str().unwrap_or("");
    Ok(json!({ "status_code": 201, "body": { "id": "123", "item": item } }))
});
```

### Registering Triggers

```rust
iii.register_trigger("http", "orders::create", json!({
    "api_path": "/orders",
    "http_method": "POST"
}))?;
```

### Invoking Functions

```rust
use iii_sdk::{TriggerRequest, TriggerAction};
use serde_json::json;

// Synchronous -- waits for the result
let result = iii.trigger(TriggerRequest {
    function_id: "orders::create".to_string(),
    payload: json!({ "body": { "item": "widget" } }),
    action: None,
    timeout_ms: None,
}).await?;

// Fire-and-forget
iii.trigger(TriggerRequest {
    function_id: "analytics::track".to_string(),
    payload: json!({ "event": "page_view" }),
    action: Some(TriggerAction::Void),
    timeout_ms: None,
}).await?;

// Async via named queue
iii.trigger(TriggerRequest {
    function_id: "orders::process".to_string(),
    payload: json!({ "order_id": "456" }),
    action: Some(TriggerAction::Enqueue { queue: "payments".to_string() }),
    timeout_ms: None,
}).await?;
```

### Stream Operations

```rust
use iii_sdk::{register_worker, InitOptions, TriggerRequest, UpdateBuilder, UpdateOp};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii = register_worker("ws://localhost:49134", InitOptions::default());

    // Set a stream item
    iii.trigger(TriggerRequest {
        function_id: "stream::set".into(),
        payload: json!({
            "stream_name": "users",
            "group_id": "active",
            "item_id": "user-1",
            "data": { "status": "online" },
        }),
        action: None,
        timeout_ms: None,
    }).await?;

    // Atomic update with UpdateBuilder
    let ops = UpdateBuilder::new()
        .increment("total", 100)
        .set("status", json!("processing"))
        .build();

    iii.trigger(TriggerRequest {
        function_id: "stream::update".into(),
        payload: json!({
            "stream_name": "orders",
            "group_id": "user-123",
            "item_id": "order-456",
            "ops": ops,
        }),
        action: None,
        timeout_ms: None,
    }).await?;

    Ok(())
}
```

### Logger

```rust
use iii_sdk::Logger;

let logger = Logger::new(Some("my-function".to_string()));
logger.info("Processing started", None);
```

The `Logger` struct emits OTel `LogRecord`s, falling back to the `tracing` crate when OTel is not
initialized.

## Resources

- [Documentation](https://iii.dev/docs)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)
