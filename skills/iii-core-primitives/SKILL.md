---
name: iii-core-primitives
description: >-
  Use when registering iii functions, binding triggers, selecting sync/void/enqueue invocation,
  authoring custom triggers, moving channel data, or adapting external HTTP functions across
  TypeScript, Python, and Rust.
---

# Core Primitives

iii has three top-level primitives:

- **Function**: a named unit of work such as `orders::validate`
- **Trigger**: an event source bound to a function
- **Worker**: a process that connects to the engine and executes functions

Use `::` in function IDs, leading slashes in HTTP `api_path`, and `expression` for cron config.

## Function Registration

Register local handlers when you control the implementation. Register HTTP-invoked functions when
iii should call an existing external endpoint.

| Shape | Use for |
| --- | --- |
| `registerFunction(id, handler, options?)` | Local worker code |
| `registerFunction(id, HttpInvocationConfig, options?)` | Existing HTTP services |
| `registerTrigger({ type, function_id, config, metadata? })` | Binding an event source |
| `trigger({ function_id, payload, action?, timeout? })` | Calling any function by ID |

Functions and triggers can carry metadata for ownership, discovery, and generated skills. Do not put
secrets in metadata.

## Built-In Trigger Shapes

| Trigger type | Registration config | Handler payload |
| --- | --- | --- |
| `http` | `{ api_path: "/orders/:id", http_method: "POST" }` | `{ query_params, path_params, headers, path, method, body }` |
| `cron` | `{ expression: "0 0 9 * * * *" }` | `{ trigger, job_id, scheduled_time, actual_time }` |
| `durable:subscriber` | `{ topic: "payments" }` | The queued message payload |
| `subscribe` | `{ topic: "orders.created" }` | The published event payload |
| `state` | `{ scope: "orders", key?: "order-123" }` | `{ event_type, scope, key, old_value, new_value }` |
| `stream` | `{ stream_name, group_id, item_id? }` | Stream event details |
| `log` | `{ level: "warn" }` | OpenTelemetry-style log data |

Add `condition_function_id` to built-in trigger config when the handler should only run if a boolean
condition function returns `true`.

## Invocation Modes

| Mode | Shape | Use when |
| --- | --- | --- |
| Sync | `trigger({ function_id, payload })` | The caller needs the result |
| Void | `TriggerAction.Void()` | Optional side effect, no result needed |
| Enqueue | `TriggerAction.Enqueue({ queue })` | Reliable async work with queue policy |

Use enqueue for work that must complete with retries. Use void for analytics, notifications, and
other non-critical side effects.

## Code Examples

### TypeScript

```typescript
import { registerWorker, TriggerAction } from "iii-sdk";

const iii = registerWorker("ws://localhost:49134", { workerName: "orders-worker" });

iii.registerFunction("orders::validate", async (order) => {
  if (!order.id) throw new Error("missing order id");
  return { ...order, valid: true };
});

iii.registerFunction("orders::process", async (order) => {
  const validated = await iii.trigger({ function_id: "orders::validate", payload: order });
  await iii.trigger({
    function_id: "orders::charge",
    payload: validated,
    action: TriggerAction.Enqueue({ queue: "payments" }),
  });
  return { accepted: true, orderId: validated.id };
});

iii.registerTrigger({
  type: "http",
  function_id: "orders::process",
  config: { api_path: "/orders", http_method: "POST" },
});
```

### Python

```python
from iii import register_worker

iii = register_worker("ws://localhost:49134")

def validate(order):
    if not order.get("id"):
        raise ValueError("missing order id")
    return {**order, "valid": True}

def process(order):
    validated = iii.trigger({"function_id": "orders::validate", "payload": order})
    iii.trigger({
        "function_id": "orders::charge",
        "payload": validated,
        "action": {"type": "enqueue", "queue": "payments"},
    })
    return {"accepted": True, "orderId": validated["id"]}

iii.register_function("orders::validate", validate)
iii.register_function("orders::process", process)
iii.register_trigger({
    "type": "http",
    "function_id": "orders::process",
    "config": {"api_path": "/orders", "http_method": "POST"},
})
```

### Rust

```rust
use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, RegisterTriggerInput, TriggerAction,
    TriggerRequest,
};
use serde_json::json;

let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default());

iii.register_function(RegisterFunction::new("orders::validate", |order: serde_json::Value| {
    if order["id"].is_null() {
        return Err("missing order id".into());
    }
    Ok(json!({ "valid": true, "order": order }))
}))?;

let process_client = iii.clone();
iii.register_function(RegisterFunction::new_async("orders::process", move |order: serde_json::Value| {
    let iii = process_client.clone();
    async move {
        let validated = iii.trigger(TriggerRequest::new("orders::validate", order)).await?;
        iii.trigger(TriggerRequest {
            function_id: "orders::charge".into(),
            payload: validated.clone(),
            action: Some(TriggerAction::Enqueue { queue: "payments".into() }),
            timeout_ms: None,
        }).await?;
        Ok(json!({ "accepted": true, "order": validated }))
    }
}))?;

iii.register_trigger(RegisterTriggerInput {
    trigger_type: "http".into(),
    function_id: "orders::process".into(),
    config: json!({ "api_path": "/orders", "http_method": "POST" }),
    metadata: None,
})?;
```

## Advanced Primitive Patterns

- **Custom triggers**: use `registerTriggerType({ id, description }, handler)` when the event source is
  not built in. Keep listener setup in `registerTrigger` and cleanup in `unregisterTrigger`.
- **Channels**: use `createChannel()` for binary or streaming data that should not be serialized into
  JSON payloads. Pass `readerRef` or `writerRef` through a function payload.
- **HTTP-invoked functions**: use `HttpInvocationConfig` for legacy APIs, third-party endpoints, or
  immutable services. Use environment variable names for auth fields, not raw secrets.
- **Schemas**: Rust can derive request/response schemas with `schemars::JsonSchema`; Python can use
  type hints or Pydantic; Node can pass JSON Schema manually.

## When to Use

- Use this skill for function registration, trigger binding, trigger payload shapes, invocation mode
  decisions, trigger conditions, custom trigger types, channels, and HTTP-invoked functions.
- Use this when a task spans TypeScript, Python, or Rust examples for the same iii primitive.

## Boundaries

- For engine ports, adapters, queue retry policy, worker manager, RBAC listeners, and deployment
  config, use `iii-engine-config`.
- For SDK-specific package exports and language caveats, use `iii-sdk-reference`.
- For complete backend designs such as workflows, CQRS, agentic systems, and reactive apps, use
  `iii-architecture-patterns`.
- For failed invocations, timeouts, RBAC denials, and retryability, use `iii-error-handling`.
- Worker-backed capability details live with the worker docs, not as top-level iii skills.
