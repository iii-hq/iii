# iii

iii is a single engine that replaces your API framework, task queue, cron scheduler, pub/sub, state store, and observability pipeline with three primitives: **Function**, **Trigger**, and **Worker**. You write functions, declare what triggers them, connect a worker, and the engine handles routing, retries, and observability.

See the [engine README](../engine/README.md) for architecture details and the [documentation](https://iii.dev/docs) for full guides.

## SDKs

[![npm](https://img.shields.io/npm/v/iii-sdk)](https://www.npmjs.com/package/iii-sdk)
[![PyPI](https://img.shields.io/pypi/v/iii-sdk)](https://pypi.org/project/iii-sdk/)
[![crates.io](https://img.shields.io/crates/v/iii-sdk)](https://crates.io/crates/iii-sdk)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

| Package                                            | Language             | Install               | Docs                                      |
| -------------------------------------------------- | -------------------- | --------------------- | ----------------------------------------- |
| [`iii-sdk`](https://www.npmjs.com/package/iii-sdk) | Node.js / TypeScript | `npm install iii-sdk` | [README](./packages/node/iii/README.md)   |
| [`iii-sdk`](https://pypi.org/project/iii-sdk/)     | Python               | `pip install iii-sdk` | [README](./packages/python/iii/README.md) |
| [`iii-sdk`](https://crates.io/crates/iii-sdk)      | Rust                 | Add to `Cargo.toml`   | [README](./packages/rust/iii/README.md)   |

## Hello World

### Node.js

```javascript
import { registerWorker } from 'iii-sdk';

const iii = registerWorker('ws://localhost:49134');

iii.registerFunction({ id: 'greet' }, async (input) => {
  return { message: `Hello, ${input.name}!` };
});

iii.registerTrigger({
  type: 'http',
  function_id: 'greet',
  config: { api_path: '/greet', http_method: 'POST' },
});

const result = await iii.trigger({ function_id: 'greet', payload: { name: 'world' } });
```

### Python

```python
from iii import register_worker

iii = register_worker("ws://localhost:49134")

def greet(data):
    return {"message": f"Hello, {data['name']}!"}

iii.register_function({"id": "greet"}, greet)

iii.register_trigger({
    "type": "http",
    "function_id": "greet",
    "config": {"api_path": "/greet", "http_method": "POST"}
})

result = iii.trigger({"function_id": "greet", "payload": {"name": "world"}})
```

### Rust

```rust
use iii_sdk::{register_worker, InitOptions, TriggerRequest, RegisterFunctionMessage, RegisterTriggerInput};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default())?;

    iii.register_function(RegisterFunctionMessage::with_id("greet".into()), |input| async move {
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("world");
        Ok(json!({ "message": format!("Hello, {name}!") }))
    });

    iii.register_trigger(RegisterTriggerInput { trigger_type: "http".into(), function_id: "greet".into(), config: json!({
        "api_path": "/greet",
        "http_method": "POST"
    }) })?;

    let result: serde_json::Value = iii
        .trigger(TriggerRequest::new("greet", json!({ "name": "world" })))
        .await?;

    Ok(())
}
```

## API

| Operation                | Node.js                                              | Python                                      | Rust                                         | Description                                            |
| ------------------------ | ---------------------------------------------------- | ------------------------------------------- | -------------------------------------------- | ------------------------------------------------------ |
| Initialize               | `registerWorker(url)`                                | `register_worker(url, options?)`            | `register_worker(url, options)`              | Create an SDK instance and auto-connect                |
| Register function        | `iii.registerFunction({ id }, handler)`              | `iii.register_function(id, handler)`        | `iii.register_function(id, \|input\| ...)`   | Register a function that can be invoked by name        |
| Register trigger         | `iii.registerTrigger({ type, function_id, config })` | `iii.register_trigger({"type": ..., "function_id": ..., "config": ...})` | `iii.register_trigger(type, fn_id, config)?` | Bind a trigger (HTTP, cron, queue, etc.) to a function |
| Invoke (await)           | `await iii.trigger({ function_id, payload })`        | `await iii.trigger({"function_id": id, "payload": data})` | `iii.trigger(TriggerRequest::new(id, data)).await?` | Invoke a function and wait for the result              |
| Invoke (fire-and-forget) | `iii.trigger({ function_id, payload, action: TriggerAction.Void() })` | Same | Same | Invoke without waiting |

`registerWorker()` / `register_worker()` creates an SDK instance and auto-connects to the engine. It handles WebSocket communication, automatic reconnection, and OpenTelemetry instrumentation. All three SDKs expose the same API surface — register functions and triggers, then invoke them.

> `call`, `callVoid`, `triggerVoid` (and Python/Rust equivalents) have been removed. Use `trigger()` for all invocations. For fire-and-forget, use `trigger({ function_id, payload, action: TriggerAction.Void() })`.

For language-specific details (modules, streams, OpenTelemetry), see the per-SDK READMEs linked in the table above.

## Development

### Prerequisites

- Node.js 20+ and pnpm (for Node.js SDK)
- Python 3.10+ and uv (for Python SDK)
- Rust 1.85+ and Cargo (for Rust SDK)
- iii engine running on `ws://localhost:49134`

### Building

```bash
cd packages/node && pnpm install && pnpm build
cd packages/python/iii && python -m build
cd packages/rust/iii && cargo build --release
```

### Testing

```bash
cd packages/node && pnpm test
cd packages/python/iii && pytest
cd packages/rust/iii && cargo test
```

## Examples

See the [Quickstart guide](https://iii.dev/docs/quickstart) for step-by-step tutorials.

## Resources

- [Documentation](https://iii.dev/docs)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)

## License

Apache 2.0
