---
name: iii-sdk-reference
description: >-
  Use when working with iii SDK APIs across Node.js, browser, Python, Rust, or Go: package
  installation, worker initialization, function/trigger registration, observers, invocation,
  local dispatch, channels, logging, OpenTelemetry, and language-specific caveats.
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

# Go
go get github.com/iii-hq/iii/sdk/packages/go/iii
```

## Choose the SDK

| SDK | Package | Best for | Important caveat |
| --- | --- | --- | --- |
| Node.js | `iii-sdk` | Server-side TypeScript/JavaScript workers | Supports custom headers, Logger, OpenTelemetry, HTTP-invoked functions |
| Browser | `iii-browser-sdk` | Web apps and interactive UI callbacks | Connect through the `rbac-proxy` worker's public port, never the engine port; keep secrets server-side |
| Python | `iii-sdk` | Sync or async Python workers | Use `trigger_async` inside async handlers |
| Rust | `iii-sdk` | High-performance tokio workers | Handler error type should map into `iii_sdk::Error` |
| Go | `github.com/iii-hq/iii/sdk/packages/go/iii` | Go services | Handlers take `(ctx, json.RawMessage)`; pass the handler's `ctx` to `Trigger` |

`Logger`/OpenTelemetry, HTTP request/response types, stream, queue, and worker-connection types live in
the helpers package, installed alongside the SDK: `@iii-dev/helpers` (Node, with submodules like
`/observability` and `/http`) or `iii-helpers` (Python `iii_helpers.*`, Rust `iii_helpers::*`).

## Common API Map

| Capability | Node / browser | Python | Rust | Go |
| --- | --- | --- | --- | --- |
| Connect worker | `registerWorker(url, options?)` | `register_worker(address, options?)` | `register_worker(url, InitOptions)` | `iii.RegisterWorker(url, opts...)` |
| Register local function | `registerFunction(id, handler, options?)` | `register_function(id, handler, **options)` | `register_function("id", RegisterFunction::new(...))` | `client.RegisterFunction(id, handler, opts...)` |
| Register trigger (deprecated in 0.25) | `registerTrigger({ type, function_id, config })` | `register_trigger({...})` | `register_trigger(RegisterTriggerInput::new(type, function_id, config))` | `client.RegisterTrigger(id, type, functionID, config)` |
| Observe a function | `observe(source, target, options?)` | `observe(source, target, options)` | `observe(Source::function(id)..., target)` / `observe_with(.., ObserverOptions)` | `client.Observe(iii.Source{...}, target, opts...)` |
| Owner hooks | `registerFunction(id, h, { observe: {...} })` | `register_function(id, h, observe=ObserveHooks(...))` | `RegisterFunction::new(h).observe(Observe::owner()...)` | `RegisterFunctionOptions{Observe: &iii.ObserveHooks{...}}` |
| Current invocation | `currentInvocation()` | `current_invocation()` | `iii_sdk::current_invocation()` | `iii.InvocationFromContext(ctx)` |
| Invoke function | `trigger({ function_id, payload })` | `trigger(request)` / `trigger_async(request)` | `trigger(TriggerRequest)` | `client.Trigger(ctx, iii.TriggerRequest{...})` |
| Durable enqueue | `TriggerAction.Enqueue({ queue })` | `{"type": "enqueue", "queue": name}` | `TriggerAction::Enqueue { queue }` | `iii.EnqueueAction(queue)` |
| Channels | `createChannel(iii)` (`iii-sdk/helpers`) | `create_channel(iii)` / `create_channel_async(iii)` (`iii.helpers`) | `iii_sdk::helpers::create_channel(&iii, None).await` | `client.CreateChannel(ctx, nil)` |

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
registration, trigger metadata, channels, observers, and owner hooks. Custom trigger types
(`registerTriggerType`) still work but are deprecated in 0.25.

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

Since 0.25 the browser `trigger` also sends `metadata`, `traceparent` and `baggage`, and handlers
receive metadata as their second argument. Browsers have no `AsyncLocalStorage`:
`iii.currentInvocation()` is only known until the handler's first `await`, so read it first and
wrap later calls in `iii.withInvocation(inv, () => ...)`.

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
`schemars::JsonSchema`. Add the `otel` feature when using OpenTelemetry helpers. `register_trigger`
and `register_trigger_type` carry `#[deprecated(since = "0.25.0")]`: calls compile with a warning
(use `#[allow(deprecated)]` where you still need them). A local call runs on the caller's task, and
a sync handler (`RegisterFunction::new`) runs inline: make a blocking handler async or opt out with
`.local_dispatch(false)`.

## Go

```go
client := iii.RegisterWorker(iii.DefaultEngineURL, iii.WithNamespace("orders"))

client.RegisterFunction("users::lookup", func(ctx context.Context, data json.RawMessage) (any, error) {
	var in struct{ UserID string `json:"userId"` }
	if err := json.Unmarshal(data, &in); err != nil {
		return nil, err
	}
	return map[string]string{"userId": in.UserID, "name": "Ada"}, nil
})
```

Per-invocation metadata rides on `ctx` (`iii.MetadataFromContext(ctx)`). Inside a handler, pass its
`ctx` to `client.Trigger` so the call carries the caller and the observe depth.

## Observers and local dispatch (0.25)

- `observe(source, target, options?)` is in every SDK; `registerTrigger` and `registerTriggerType`
  are deprecated (one warning per process, still working). The built-in providers (`http`, `cron`,
  queue, `state`, `stream`, pub/sub, database) have not migrated, so keep `registerTrigger` for
  them. Details and examples per language: `iii-observe`.
- A call to a function the same worker registered runs in-process once the engine confirmed it.
  Force the engine path per call with `dispatch: 'remote'` (Node, browser, Python
  `"dispatch": "remote"`), Rust `TriggerRequest::new(..).dispatch(Dispatch::Remote)`, Go
  `Dispatch: iii.DispatchRemote`; or per function with `localDispatch: false`
  (`local_dispatch=False`, `.local_dispatch(false)`, `LocalDispatch: &off`).
- `registerFunction` returns a `FunctionRef` with `isObserved()` / `observers()` (Python
  `is_observed()`, Rust `is_observed()`, Go `client.IsObserved(id)`).

## Channels

- Use channels for binary data, large payloads, or streaming transfer between workers.
- Pass `readerRef` or `writerRef` through a function payload.
- Reconstruct readers/writers from refs in consumers when the SDK requires it.
- A close tells the reader the stream is complete, so a writer that fails must abort instead: Node
  `writer.stream.destroy()` (no argument), Rust drop the writer without `close()`, Go `Close` after
  a failed `Write`. Go, Python and browser writers have no other abort call. A crash or a lost
  connection aborts on its own; a writer left open keeps the reader waiting until its process exits.
- On an abort the read errors after the data already sent (Rust `Err`, Node stream `'error'`,
  browser rejection, Python `ConnectionError`, Go non-EOF error): treat it as an incomplete
  transfer. A writer whose socket never opened (it failed before its first write or, in Node, right
  after one, while the socket was still connecting) never reached the engine, so also check the
  producer's result.

## Namespaces

A worker belongs to one namespace: `options.namespace` (`InitOptions.namespace`) → the `III_NAMESPACE`
environment variable → the engine's `default`. Compose sets `III_NAMESPACE` to its daemon's namespace
(`iii compose -n dev ...`) for every worker it starts, so a whole project lands in one namespace
without any code change. Routing is strict: a function is only reachable in the namespace it
registered in.

- `iii.trigger({ function_id })` resolves in the calling worker's namespace. Calls to your own
  functions and to other workers declared in the same `worker-compose.yaml` need no namespace.
- Engine-owned functions register in `default`: `engine::*`, `configuration::*`, `stream::*`
  (from `iii-stream`) and the `observability::*` sources. `engine::*` and `configuration::*` resolve
  there implicitly (since 0.24.5); for the others pass `namespace: "default"` on the call, or on an
  `observe` source, when your worker runs in a Compose namespace.
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

// Engine-owned configuration worker: resolves in `default` without a namespace;
// naming it explicitly is equivalent
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
let cfg = iii
    .trigger(TriggerRequest::new("configuration::get", json!({ "id": "orders" })).namespace("default"))
    .await?;
```

## When to Use

- Use this skill for package names, SDK exports, initialization options, browser security constraints,
  channel API details, and language-specific syntax.
- Use this when a task asks for Python, Rust, or Go examples and the issue is SDK syntax rather
  than iii architecture.

## Boundaries

- For the common Function/Trigger/Worker model, built-in trigger schemas, custom triggers, and
  invocation mode decisions, use `iii-core-primitives`.
- For the observe model, filters, owner hooks, and the trigger migration, use `iii-observe`.
- For deployment config, engine-owned workers, RBAC (`rbac-proxy`), and ports, use
  `iii-engine-config`.
- For retryability and exception classes, use `iii-error-handling`.
