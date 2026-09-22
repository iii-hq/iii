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
| Browser | `iii-browser-sdk` | Web apps and interactive UI callbacks | Connect through the `rbac-proxy` worker's public port, never the engine port; keep secrets server-side |
| Python | `iii-sdk` | Sync or async Python workers | Use `trigger_async` inside async handlers |
| Rust | `iii-sdk` | High-performance tokio workers | Handler error type should map into `iii_sdk::Error` |

`Logger`/OpenTelemetry, HTTP request/response types, stream, queue, and worker-connection types live in
the helpers package — `@iii-dev/helpers` (Node, with submodules like `/observability` and `/http`) or
`iii-helpers` (Python `iii_helpers.*`, Rust `iii_helpers::*`) — installed alongside the SDK.

## Common API Map

| Capability | Node | Python | Rust |
| --- | --- | --- | --- |
| Connect worker | `registerWorker(url, options?)` | `register_worker(address, options?)` | `register_worker(url, InitOptions)` |
| Register local function | `registerFunction(id, handler, options?)` | `register_function(id, handler, **options)` | `register_function("id", RegisterFunction::new(...))` |
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

Do not expose the private engine worker port to untrusted browsers; put the `rbac-proxy` worker in front of it (`iii trigger compose::add worker=rbac-proxy`). Browser workers cannot send
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
and `iii.trigger(request)` in sync contexts. `HttpResponse` (from `iii_helpers.http`) uses `status_code`, like the other helpers packages.

## Rust

```rust
use iii_sdk::{register_worker, InitOptions, RegisterFunction};
use serde_json::json;

let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default());

iii.register_function(
    "users::lookup",
    RegisterFunction::new(|input: serde_json::Value| -> Result<serde_json::Value, iii_sdk::Error> {
        Ok(json!({ "userId": input["userId"], "name": "Ada" }))
    }).description("Look up a user"),
);
```

Rust supports typed handlers and schema extraction when input/output types derive
`schemars::JsonSchema`. Add the `otel` feature when using OpenTelemetry helpers.

## Channels

- Use channels for binary data, large payloads, or streaming transfer between workers.
- Pass `readerRef` or `writerRef` through a function payload.
- Reconstruct readers/writers from refs in consumers when the SDK requires it.

## Namespaces

A worker belongs to one namespace: `options.namespace` (`InitOptions.namespace`) → the `III_NAMESPACE`
environment variable → the engine's `default`. Compose sets `III_NAMESPACE` to its daemon's namespace
(`iii compose -n dev ...`) for every worker it starts, so a whole project lands in one namespace
without any code change. Routing is strict: a function is only reachable in the namespace it
registered in.

- `iii.trigger({ function_id })` resolves in the calling worker's namespace. Calls to your own
  functions and to other workers declared in the same `worker-compose.yaml` need no namespace.
- Engine-owned functions register in `default`: `engine::*`, `configuration::*`, and `stream::*`
  (from `iii-stream`). `engine::*` resolves there implicitly; for the others pass
  `namespace: "default"` on the call when your worker runs in a Compose namespace.
- `registerTrigger` binds in the worker's namespace. Leave `trigger_namespace` unset; the engine
  looks for the trigger type's provider in your namespace first and the engine's own second, which
  is what lets a project ship its own `http` provider or fall back to the engine's `cron`.
- Never prefix a function id with a namespace. The id stays `orders::validate`; the namespace is a
  separate field.
- From the CLI, `iii trigger -n dev orders::validate ...` selects the namespace; omitting `-n`
  resolves in `default`.

```typescript
// Same-project worker: no namespace
await iii.trigger({ function_id: "orders::validate", payload: order });

// Engine-owned configuration worker from a namespaced project
const cfg = await iii.trigger({
  function_id: "configuration::get",
  namespace: "default",
  payload: { id: "orders" },
});
```

```python
cfg = await iii.trigger_async(
    {"function_id": "configuration::get", "namespace": "default", "payload": {"id": "orders"}}
)
```

```rust
let cfg = iii.trigger(TriggerRequest {
    function_id: "configuration::get".into(),
    namespace: Some("default".into()),
    payload: json!({ "id": "orders" }),
    ..Default::default()
}).await?;
```

## When to Use

- Use this skill for package names, SDK exports, initialization options, browser security constraints,
  channel API details, and language-specific syntax.
- Use this when a task asks for Python or Rust examples and the issue is SDK syntax rather than iii
  architecture.

## Boundaries

- For the common Function/Trigger/Worker model, built-in trigger schemas, custom triggers, and
  invocation mode decisions, use `iii-core-primitives`.
- For deployment config, engine-owned workers, RBAC (`rbac-proxy`), and ports, use
  `iii-engine-config`.
- For retryability and exception classes, use `iii-error-handling`.
