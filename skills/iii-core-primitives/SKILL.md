---
name: iii-core-primitives
description: >-
  Use when registering iii functions, calling them (call with sync, void, or enqueue), observing
  functions and event sources, creating workers, inspecting the live worker registry, installing
  registry workers, moving channel data, or adapting external HTTP functions across TypeScript,
  Python, and Rust.
---

# Core Primitives

iii has three primitives:

- **Worker**: a process that connects to the engine, registers functions, and calls them
- **Function**: a named unit of work such as `orders::validate`; any worker runs it with `call`
- **Observer**: `iii.observe(a, b)`: when `a` runs, `b` runs

Everything is reactive. An HTTP request, a cron tick, a queue message, and a state change are each a
call of a source function (`http::on-request`, `cron::on-tick`, `queue::on-message`,
`state::on-changed`), so observing that function is how work starts on the event. See `iii-observe`.

Use `::` in function IDs, leading slashes in HTTP `path` args, and `expression` for cron args.

## Function Registration

Register local handlers when you control the implementation. Register HTTP-invoked functions when
iii should call an existing external endpoint.

| Shape | Use for |
| --- | --- |
| `registerFunction(id, handler, options?)` | Local worker code |
| `registerFunction(id, HttpInvocationConfig, options?)` | Existing HTTP services |
| `observe(source, target, options?)` | Running `target` when `source` runs: another function, an HTTP route, a cron schedule, a queue topic, an engine event (`iii-observe`) |
| `call({ function_id, payload, action?, timeoutMs? })` | Calling any function by ID |

Functions and observers can carry metadata for ownership, discovery, and generated skills. Do not
put secrets in metadata.

## Workers and Registry

A worker is any process that connects to the engine and registers functions. There are two common
paths:

| Task | Use |
| --- | --- |
| Create your own worker | Write SDK code that calls `registerWorker`, `registerFunction`, `call`, and `observe` |
| Add an existing capability | Browse `https://workers.iii.dev/`, then call `compose::add worker=<name>` |
| Pin a worker version | `compose::add worker=<name>@<version>` |
| Declare a local worker | Add `worker: path://./workers/my-worker` under `containers:` |
| Reproduce a project | Commit the exact versions in `worker-compose.yaml` |

The public worker registry at `workers.iii.dev` is for installable workers such as HTTP, state,
queue, pub/sub, cron, database, shell, console, rbac-proxy, and other capability workers.
(`iii-sandbox` and `iii-observability` are engine-owned and are not added through the registry.)
Those workers may ship their own function-level skills; do not duplicate every capability as a
top-level iii skill.

### Worker Manifest

Use `iii.worker.yaml` when iii should start a local worker project:

```yaml
name: math-worker
runtime:
  kind: python
  package_manager: pip
  entry: math_worker.py
scripts:
  install: "pip install -r requirements.txt"
  start: "python math_worker.py"
```

The manifest describes how to start the process. Once running, the WebSocket connection and function
registrations are what make the worker part of iii.

### Live Engine Registry

The engine keeps a live registry of connected workers, registered functions, and observers. Read it
through the built-in discovery functions:

| Function | Returns |
| --- | --- |
| `engine::workers::list` | Connected workers and metrics |
| `engine::functions::list` | Registered functions, with `args_format`, `private`, `observable`, and `observer_count` |
| `engine::observers::list` | Observers you may see (`function_id`, `namespace`, `worker` filters) |

For topology changes, observe the engine-hosted sources `engine::on-workers-changed`,
`engine::on-functions-changed`, or `engine::on-observers-changed` with `deliver: 'input'` (see
`iii-observe`).

## Built-In Sources

Capability workers host these source functions, and the engine hosts `observability::on-log`. Add
the worker, then observe the function with the args it takes; `iii-observe` has the full table and
the other engine-hosted sources.

| Source function | `args` | What the target receives |
| --- | --- | --- |
| `http::on-request` | `{ method: "POST", path: "/orders/:id", middleware_function_ids? }` | `{ method, path, path_params, query_params, headers, body }`; its result is the response `{ status_code, headers?, body? }` |
| `cron::on-tick` | `{ expression: "0 0 9 * * * *" }` | The envelope; `input` is `{ expression, scheduled_time, actual_time }` |
| `queue::on-message` | `{ topic: "payments", max_retries?, backoff_ms?, group? }` | The published data; returning acks the message |
| `state::on-changed` | none; filter on `input.scope`, `input.key` | The envelope; `input` is `{ event_type, scope, key, old_value, new_value }` |
| `database::on-row-changed` | `{ db, table?, ops? }` | The envelope; `input` is the row change |
| `observability::on-log` | none; filter on `input.severity_text`; from a namespaced worker, observe with `namespace: "default"` | The envelope; `input` is one log record |

A condition you can state over the call (its input, output, error, caller, or metadata) is a
filter on the observer. A condition that needs code goes in the target.

## Invocation Modes

| Mode | Shape | Use when |
| --- | --- | --- |
| Sync | `call({ function_id, payload })` | The caller needs the result |
| Void | `CallAction.Void()` | Optional side effect, no result needed |
| Enqueue | `CallAction.Enqueue({ queue })` | Reliable async work with queue policy |

Use enqueue for work that must complete with retries. Use void for analytics, notifications, and
other non-critical side effects.

## Namespaces

Every worker registers in a namespace: the `namespace` init option, else `III_NAMESPACE`, else the
engine's `default`. Compose sets `III_NAMESPACE` to its daemon's namespace for every worker it
starts, so all workers in one `worker-compose.yaml` share a namespace and call each other by bare
function id. Engine-owned functions (`engine::*`, `configuration::*`, `observability::*`,
`stream::*`) live in `default`; `engine::*` and `configuration::*` resolve there implicitly, and
`observability::*` and `stream::*` need `namespace: "default"` on the call or observer from a
namespaced worker. Routing is strict and ids are never prefixed with a namespace. From the CLI,
`iii call -n <namespace> <function>` selects it. Details and per-language examples are in
`iii-sdk-reference`.

## Code Examples

### TypeScript

```typescript
import { registerWorker, CallAction } from "iii-sdk";

const iii = registerWorker("ws://localhost:49134", { workerName: "orders-worker" });

iii.registerFunction("orders::validate", async (order) => {
  if (!order.id) throw new Error("missing order id");
  return { ...order, valid: true };
});

iii.registerFunction("orders::process", async (req) => {
  const validated = await iii.call({ function_id: "orders::validate", payload: req.body });
  await iii.call({
    function_id: "orders::charge",
    payload: validated,
    action: CallAction.Enqueue({ queue: "payments" }),
  });
  return { status_code: 202, body: { accepted: true, orderId: validated.id } };
});

// POST /orders runs orders::process; its result is the HTTP response
iii.observe(
  { function_id: "http::on-request", args: { method: "POST", path: "/orders" } },
  "orders::process",
);
```

### Python

```python
from iii import register_worker

iii = register_worker("ws://localhost:49134")

def validate(order):
    if not order.get("id"):
        raise ValueError("missing order id")
    return {**order, "valid": True}

def process(req):
    validated = iii.call({"function_id": "orders::validate", "payload": req["body"]})
    iii.call({
        "function_id": "orders::charge",
        "payload": validated,
        "action": {"type": "enqueue", "queue": "payments"},
    })
    return {"status_code": 202, "body": {"accepted": True, "orderId": validated["id"]}}

iii.register_function("orders::validate", validate)
iii.register_function("orders::process", process)
iii.observe(
    {"function_id": "http::on-request", "args": {"method": "POST", "path": "/orders"}},
    "orders::process",
)
```

### Rust

```rust
use iii_sdk::{register_worker, CallAction, Error, InitOptions, RegisterFunction, Source};
use iii_sdk::protocol::CallRequest;
use serde_json::{json, Value};

let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default());

iii.register_function("orders::validate", RegisterFunction::new(|order: Value| -> Result<Value, Error> {
    if order["id"].is_null() {
        return Err("missing order id".into());
    }
    Ok(json!({ "valid": true, "order": order }))
}));

let process_client = iii.clone();
iii.register_function("orders::process", RegisterFunction::new_async(move |req: Value| {
    let iii = process_client.clone();
    async move {
        let validated = iii.call(CallRequest::new("orders::validate", req["body"].clone())).await?;
        iii.call(CallRequest {
            function_id: "orders::charge".into(),
            payload: validated.clone(),
            action: Some(CallAction::Enqueue { queue: "payments".into() }),
            timeout_ms: None,
        }).await?;
        Ok::<_, Error>(json!({ "status_code": 202, "body": { "accepted": true, "order": validated } }))
    }
}));

// POST /orders runs orders::process; its result is the HTTP response
iii.observe(
    Source::function("http::on-request").args(json!({ "method": "POST", "path": "/orders" })),
    "orders::process",
);
```

## Advanced Primitive Patterns

- **Event sources**: a worker turns events from outside iii into calls of a function it hosts,
  registered with a handler that returns the event and `{ args_format, private: true }`. It
  overrides iii's defaults with `onObserverAdded` (set up from `obs.args`), `onObserverRemoved`
  (tear down), `onObserverMatch` (which observers want each call), and `onObserverDeliver`
  (deliver each call itself and return what the caller gets). See `iii-observe`.
- **Channels**: use `createChannel()` for binary or streaming data that should not be serialized into
  JSON payloads. Pass `readerRef` or `writerRef` through a function payload.
- **HTTP-invoked functions**: use `HttpInvocationConfig` for legacy APIs, third-party endpoints, or
  immutable services. Use environment variable names for auth fields, not raw secrets.
- **Schemas**: Rust can derive request/response schemas with `schemars::JsonSchema`; Python can use
  type hints or Pydantic; Node can pass JSON Schema manually.

## When to Use

- Use this skill for function registration, calling functions, invocation mode decisions, worker
  creation, worker registry access, the built-in source functions and their args, channels, and
  HTTP-invoked functions.
- Use this when a task spans TypeScript, Python, or Rust examples for the same iii primitive.

## Boundaries

- For `iii.observe` options, filters, engine-hosted sources, observer hooks, event sources, and
  local dispatch, use `iii-observe`.
- For engine ports, adapters, engine-owned workers, RBAC (`rbac-proxy`), and deployment config,
  use `iii-engine-config`.
- For SDK-specific package exports and language caveats, use `iii-sdk-reference`.
- For complete backend designs such as workflows, CQRS, agentic systems, and reactive apps, use
  `iii-architecture-patterns`.
- For failed invocations, timeouts, RBAC denials, and retryability, use `iii-error-handling`.
- Worker-backed capability details live with the worker docs, not as top-level iii skills.
