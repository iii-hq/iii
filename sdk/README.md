# iii SDKs

These are iii official SDKs for Node, the browser, Python, Rust, and Go. See the [engine README](../engine/README.md) for architecture details and the [documentation](https://iii.dev/docs) for full guides.

## SDKs

[![npm](https://img.shields.io/npm/v/iii-sdk)](https://www.npmjs.com/package/iii-sdk)
[![PyPI](https://img.shields.io/pypi/v/iii-sdk)](https://pypi.org/project/iii-sdk/)
[![crates.io](https://img.shields.io/crates/v/iii-sdk)](https://crates.io/crates/iii-sdk)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

| Package                                            | Language             | Install               | Docs                                      |
| -------------------------------------------------- | -------------------- | --------------------- | ----------------------------------------- |
| [`iii-sdk`](https://www.npmjs.com/package/iii-sdk) | Node.js / TypeScript | `pnpm add iii-sdk` or `npm install iii-sdk` | [README](./packages/node/iii/README.md)   |
| [`iii-browser-sdk`](https://www.npmjs.com/package/iii-browser-sdk) | Browser / TypeScript | `pnpm add iii-browser-sdk` | [README](./packages/node/iii-browser/README.md) |
| [`iii-sdk`](https://pypi.org/project/iii-sdk/)     | Python               | `pip install iii-sdk` | [README](./packages/python/iii/README.md) |
| [`iii-sdk`](https://crates.io/crates/iii-sdk)      | Rust                 | Add to `Cargo.toml`   | [README](./packages/rust/iii/README.md)   |
| [`iii-sdk`](./packages/go/iii)                         | Go                   | `go get github.com/iii-hq/iii/sdk/packages/go/iii` | [README](./packages/go/iii/README.md) |

## Hello World

Each example registers `hello::greet`, serves it on `POST /greet` by observing the http worker's
`http::on-request` (`args` are the route), and calls it. A target of `http::on-request` receives
the request (`{ path, method, body, headers, ... }`) and returns `{ status_code, body }`.

### Node.js

```javascript
import { registerWorker } from 'iii-sdk';

const iii = registerWorker('ws://localhost:49134');

iii.registerFunction('hello::greet', async (req) => {
  return { status_code: 200, body: { message: `Hello, ${req.body.name}!` } };
});

// POST /greet runs hello::greet
iii.observe(
  { function_id: 'http::on-request', args: { method: 'POST', path: '/greet' } },
  'hello::greet',
);

const result = await iii.call({ function_id: 'hello::greet', payload: { body: { name: 'world' } } });
```

### Python

```python
from iii import register_worker

iii = register_worker("ws://localhost:49134")

def greet(req):
    return {"status_code": 200, "body": {"message": f"Hello, {req['body']['name']}!"}}

iii.register_function("hello::greet", greet)

# POST /greet runs hello::greet
iii.observe(
    {"function_id": "http::on-request", "args": {"method": "POST", "path": "/greet"}},
    "hello::greet",
)

result = iii.call({"function_id": "hello::greet", "payload": {"body": {"name": "world"}}})
```

### Rust

```rust
use iii_sdk::{register_worker, CallRequest, Error, InitOptions, RegisterFunction, Source};
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default());

    iii.register_function(
        "hello::greet",
        RegisterFunction::new_async(|req: Value| async move {
            let name = req["body"]["name"].as_str().unwrap_or("world");
            Ok::<_, Error>(json!({ "status_code": 200, "body": { "message": format!("Hello, {name}!") } }))
        }),
    );

    // POST /greet runs hello::greet
    iii.observe(
        Source::function("http::on-request").args(json!({ "method": "POST", "path": "/greet" })),
        "hello::greet",
    );

    let result: Value = iii
        .call(CallRequest::new("hello::greet", json!({ "body": { "name": "world" } })))
        .await?;
    println!("result: {result}");

    Ok(())
}
```

### Go

```go
package main

import (
	"context"
	"encoding/json"
	"log"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

func main() {
	client := iii.RegisterWorker("ws://127.0.0.1:49134")

	greet, err := client.RegisterFunction("hello::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
		var req struct {
			Body struct {
				Name string `json:"name"`
			} `json:"body"`
		}
		if err := json.Unmarshal(data, &req); err != nil {
			return nil, err
		}
		return map[string]any{
			"status_code": 200,
			"body":        map[string]string{"message": "Hello, " + req.Body.Name + "!"},
		}, nil
	})
	if err != nil {
		log.Fatal(err)
	}

	// POST /greet runs hello::greet
	if _, err := client.Observe(iii.Source{
		FunctionID: "http::on-request",
		Args:       iii.Args{"method": "POST", "path": "/greet"},
	}, greet.ID); err != nil {
		log.Fatal(err)
	}

	if err := client.Connect(context.Background()); err != nil {
		log.Fatal(err)
	}
	defer func() {
		// Graceful stop: buffered reports and telemetry get up to five seconds.
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := client.Shutdown(ctx); err != nil {
			log.Printf("shutdown cut short: %v", err)
		}
	}()

	result, err := client.Call(context.Background(), iii.CallRequest{
		FunctionID: greet.ID,
		Payload:    json.RawMessage(`{"body":{"name":"world"}}`),
	})
	if err != nil {
		log.Printf("call: %v", err)
		return
	}
	log.Printf("result: %s", result)
}
```

## API

| Operation                | Node.js                                              | Python                                      | Rust                                         | Go                                          | Description                                            |
| ------------------------ | ---------------------------------------------------- | ------------------------------------------- | -------------------------------------------- | ------------------------------------------- | ------------------------------------------------------ |
| Initialize               | `registerWorker(url)`                                | `register_worker(url, options?)`            | `register_worker(url, options)`              | `iii.RegisterWorker(url)`                   | Create an SDK instance and auto-connect                |
| Register function        | `iii.registerFunction(id, handler, options?)`        | `iii.register_function(id, handler, **options)` | `iii.register_function(id, RegisterFunction::new_async(f))` | `client.RegisterFunction(id, handler, opts...)` | Register a function that can be called by id and observed |
| Call (await)             | `await iii.call({ function_id, payload })`           | `iii.call({"function_id": id, "payload": data})`, `await iii.call_async(...)` | `iii.call(CallRequest::new(id, data)).await?` | `client.Call(ctx, iii.CallRequest{FunctionID, Payload})` | Call a function and wait for the result              |
| Call (fire-and-forget)   | `iii.call({ function_id, payload, action: CallAction.Void() })` | `iii.call({"function_id": id, "payload": data, "action": CallAction.Void()})` | `iii.call(CallRequest::new(id, data).action(CallAction::Void)).await?` | `client.Call(ctx, iii.CallRequest{FunctionID, Payload, Action: iii.VoidAction()})` | Call without waiting |
| Observe                  | `iii.observe('fn::id' \| { function_id, args?, filters? }, target, options?)` | `iii.observe("fn::id" \| {"function_id": ..., "args": ..., "filters": ...}, target, options?)` | `iii.observe(Source::function(id).args(..).filters(..), target)` | `client.Observe(iii.Source{FunctionID, Args, Filters}, target, opts...)`, `client.ObserveFunction(id, target, opts...)` | Run `target` when a function runs: `args` for the function's worker, `filters` for which calls |
| Observer hooks           | `iii.onObserverAdded`, `onObserverRemoved`, `onObserverMatch`, `onObserverDeliver`; the same on the `registerFunction` result | `iii.on_observer_added`, `on_observer_removed`, `on_observer_match`, `on_observer_deliver` | `iii.on_observer_added_for(id, h)`, `..._removed_for`, `..._match_for`, `..._deliver_for` | `client.OnObserverAddedFor(id, h)`, `...RemovedFor`, `...MatchFor`, `...DeliverFor`; `ref.OnObserver*` | What the worker that hosts a function does when it is observed |

`registerWorker()` / `register_worker()` creates an SDK instance and auto-connects to the engine. It handles WebSocket communication, automatic reconnection, and OpenTelemetry instrumentation. Every SDK exposes the same API: register functions, call them, and observe them.

`trigger` (Python `trigger` / `trigger_async`, Go `client.Trigger`), `TriggerRequest` and `TriggerAction` are `call`, `CallRequest` and `CallAction` under their 0.25 names.

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
