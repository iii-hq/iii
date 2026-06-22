---
name: iii-sdk-reference
description: >-
  Use when working with iii SDK APIs across Node.js, browser, Python, or Rust: package
  installation, worker initialization, function/trigger registration, invocation, channels,
  logging, OpenTelemetry, and language-specific caveats.
---

# SDK Reference

Use this skill for language-specific SDK details. Use `iii-core-primitives` for the common model and
`iii-error-handling` for exception handling.

## Install

```bash
# TypeScript / Node.js
npm install iii-sdk

# Browser apps
npm install iii-browser-sdk

# Python
pip install iii-sdk

# Rust
cargo add iii-sdk
```

## Choose the SDK

| SDK | Package | Best for | Important caveat |
| --- | --- | --- | --- |
| Node.js | `iii-sdk` | Server-side TypeScript/JavaScript workers | Supports custom headers, Logger, OpenTelemetry, HTTP-invoked functions |
| Browser | `iii-browser-sdk` | Web apps and interactive UI callbacks | Connect through an RBAC-protected listener; keep secrets server-side |
| Python | `iii-sdk` | Sync or async Python workers | Use `trigger_async` inside async handlers |
| Rust | `iii-sdk` | High-performance tokio workers | Handler error type should map into `iii_sdk::Error` |

`Logger`/OpenTelemetry, HTTP request/response types, stream, queue, and worker-connection types live in
the helpers package — `@iii-dev/helpers` (Node, with submodules like `/observability` and `/http`) or
`iii-helpers` (Python `iii_helpers.*`, Rust `iii_helpers::*`) — installed alongside the SDK.

## Common API Map

| Capability | Node | Python | Rust |
| --- | --- | --- | --- |
| Connect worker | `registerWorker(url, options?)` | `register_worker(address, options?)` | `register_worker(url, InitOptions)` |
| Register local function | `registerFunction(id, handler, options?)` | `register_function(id, handler, **options)` | `register_function(RegisterFunction::new(...))` |
| Register trigger | `registerTrigger({ type, function_id, config })` | `register_trigger({...})` | `register_trigger(RegisterTriggerInput { ... })` |
| Invoke function | `trigger({ function_id, payload })` | `trigger(request)` / `trigger_async(request)` | `trigger(TriggerRequest)` |
| Durable enqueue | `TriggerAction.Enqueue({ queue })` | `{"type": "enqueue", "queue": name}` | `TriggerAction::Enqueue { queue }` |
| Channels | `createChannel()` | `create_channel()` / `create_channel_async()` | `create_channel(None).await` |

## Node.js

```typescript
import { registerWorker } from "iii-sdk";
import { Logger } from "@iii-dev/helpers/observability";

const iii = registerWorker("ws://localhost:49134", {
  workerName: "node-worker",
  invocationTimeoutMs: 30000,
});

iii.registerFunction("users::lookup", async (input) => {
  new Logger().info("looking up user", { userId: input.userId });
  return { userId: input.userId, name: "Ada" };
});
```

Node supports custom WebSocket headers, `Logger`, OpenTelemetry options, HTTP-invoked function
registration, trigger metadata, channels, and custom trigger types.

## Browser

```typescript
import { registerWorker, TriggerAction } from "iii-browser-sdk";

const iii = registerWorker("wss://api.example.com/worker?token=session-token");

const result = await iii.trigger({
  function_id: "backend::get-user",
  payload: { userId: "123" },
});

await iii.trigger({
  function_id: "analytics::track",
  payload: { event: "page_view" },
  action: TriggerAction.Void(),
});
```

Do not expose the private engine worker port to untrusted browsers. Browser workers cannot send
custom WebSocket headers and must not hold backend secrets.

## Python

```python
from iii import InitOptions, register_worker
from iii_helpers.observability import Logger

iii = register_worker(
    address="ws://localhost:49134",
    options=InitOptions(worker_name="python-worker"),
)

def lookup_user(data):
    Logger().info("looking up user", {"userId": data["userId"]})
    return {"userId": data["userId"], "name": "Ada"}

iii.register_function("users::lookup", lookup_user)
```

Python handlers may be sync or async. Use `await iii.trigger_async(request)` inside async handlers,
and `iii.trigger(request)` in sync contexts. `HttpResponse` (from `iii_helpers.http`) uses camelCase `statusCode`.

## Rust

```rust
use iii_sdk::{register_worker, InitOptions, RegisterFunction};
use serde_json::json;

let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default());

iii.register_function(
    RegisterFunction::new("users::lookup", |input: serde_json::Value| {
        Ok(json!({ "userId": input["userId"], "name": "Ada" }))
    }).description("Look up a user"),
)?;
```

Rust supports typed handlers and schema extraction when input/output types derive
`schemars::JsonSchema`. Add the `otel` feature when using OpenTelemetry helpers.

## Channels

- Use channels for binary data, large payloads, or streaming transfer between workers.
- Pass `readerRef` or `writerRef` through a function payload.
- Reconstruct readers/writers from refs in consumers when the SDK requires it.

## When to Use

- Use this skill for package names, SDK exports, initialization options, browser security constraints,
  channel API details, and language-specific syntax.
- Use this when a task asks for Python or Rust examples and the issue is SDK syntax rather than iii
  architecture.

## Boundaries

- For the common Function/Trigger/Worker model, built-in trigger schemas, custom triggers, and
  invocation mode decisions, use `iii-core-primitives`.
- For deployment config, queue adapter policy, worker manager, RBAC listeners, and ports, use
  `iii-engine-config`.
- For retryability and exception classes, use `iii-error-handling`.
