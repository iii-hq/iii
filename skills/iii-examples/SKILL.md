---
name: iii-examples
description: >-
  Provides concrete, copyable iii examples for agents generating code with the
  core primitives: workers register functions and triggers, functions invoke
  other functions with trigger(), TriggerAction.Enqueue({ queue }) runs durable
  async work, reactive triggers handle state changes, channels move stream-shaped
  data, and registry workers add capabilities. Use whenever an agent needs a
  working iii code shape before choosing a deeper iii skill.
---

# iii Examples

Use these examples when you need a concrete starting point. Keep the primitive model simple:
workers connect to the engine, functions contain behavior, triggers deliver events to functions, and
`trigger()` invokes functions across workers.

## Example Map

| Need | Start with |
| ---- | ---------- |
| REST endpoint | HTTP function + HTTP trigger |
| Background job | `TriggerAction.Enqueue({ queue })` |
| Optional side effect | `TriggerAction.Void()` |
| "When X changes, do Y" | State trigger |
| Large or binary payload | Channel refs |
| Worker-backed capability | Registry lookup, install, then load worker skill |

## HTTP Function And Trigger

Use this when the user wants a REST endpoint backed by an iii function.

```typescript
import { Logger, registerWorker } from "iii-sdk";

const iii = registerWorker(process.env.III_ENGINE_URL ?? "ws://localhost:49134", {
  workerName: "orders-api",
});

iii.registerFunction(
  "orders::create",
  async (input) => {
    const logger = new Logger();
    logger.info("Creating order", { customerId: input.customer_id });

    return {
      order_id: `order_${Date.now()}`,
      status: "accepted",
      customer_id: input.customer_id,
    };
  },
  { description: "Create an order" },
);

iii.registerTrigger({
  type: "http",
  function_id: "orders::create",
  config: { api_path: "/orders", http_method: "POST" },
});
```

## Same HTTP Shape In Python

Use this when the worker is written with the Python SDK.

```python
from iii import InitOptions, Logger, register_worker

iii = register_worker(
    address="ws://localhost:49134",
    options=InitOptions(worker_name="orders-api"),
)

def create_order(data):
    logger = Logger()
    logger.info("Creating order", {"customerId": data["customer_id"]})
    return {
        "order_id": f"order_{data['customer_id']}",
        "status": "accepted",
        "customer_id": data["customer_id"],
    }

iii.register_function(
    "orders::create",
    create_order,
    description="Create an order",
)

iii.register_trigger({
    "type": "http",
    "function_id": "orders::create",
    "config": {"api_path": "/orders", "http_method": "POST"},
})
```

## Same HTTP Shape In Rust

Use this when the worker is written with the Rust SDK.

```rust
use iii_sdk::{register_worker, InitOptions, Logger, RegisterFunction, RegisterTriggerInput};
use serde_json::json;

let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default());

iii.register_function(
    RegisterFunction::new("orders::create", |input: serde_json::Value| -> Result<serde_json::Value, String> {
        let logger = Logger::new();
        let customer_id = input["customer_id"].as_str().unwrap_or("unknown");
        logger.info("Creating order", Some(json!({ "customerId": customer_id })));
        Ok(json!({
            "order_id": format!("order_{}", customer_id),
            "status": "accepted",
            "customer_id": customer_id,
        }))
    }).description("Create an order"),
);

iii.register_trigger(RegisterTriggerInput {
    trigger_type: "http".into(),
    function_id: "orders::create".into(),
    config: json!({ "api_path": "/orders", "http_method": "POST" }),
    metadata: None,
})?;
```

## Durable Enqueue

Use enqueue when work should return quickly, survive process restarts, and retry through queue
configuration. The caller receives a receipt, not the handler result.

```typescript
import { Logger, TriggerAction, registerWorker } from "iii-sdk";

const iii = registerWorker(process.env.III_ENGINE_URL ?? "ws://localhost:49134", {
  workerName: "email-worker",
});

iii.registerFunction("email::send", async (input) => {
  const logger = new Logger();
  logger.info("Sending email", { to: input.to, template: input.template });
  return { sent: true };
});

iii.registerFunction("orders::notify", async (order) => {
  const receipt = await iii.trigger({
    function_id: "email::send",
    payload: {
      to: order.customer_email,
      template: "order-confirmation",
      order_id: order.order_id,
    },
    action: TriggerAction.Enqueue({ queue: "emails" }),
  });

  return { accepted: true, email_receipt: receipt.messageReceiptId };
});
```

## Reactive State Trigger

Use a state trigger when the requirement says "when X changes, do Y". Prefer reactive triggers or
streams for change-driven work instead of timer loops.

```typescript
import { Logger, registerWorker } from "iii-sdk";

const iii = registerWorker(process.env.III_ENGINE_URL ?? "ws://localhost:49134", {
  workerName: "order-reactions",
});

iii.registerFunction("orders::on-status-change", async (event) => {
  const logger = new Logger();
  const current = event.new_value?.status;
  const previous = event.old_value?.status;

  if (current === previous) {
    return { changed: false };
  }

  logger.info("Order status changed", {
    key: event.key,
    previous,
    current,
    eventType: event.event_type,
  });

  return { changed: true };
});

iii.registerTrigger({
  type: "state",
  function_id: "orders::on-status-change",
  config: { scope: "orders", key: "status" },
});
```

## Channel Handoff

Use channels for stream-shaped or binary data that should not be serialized into a JSON payload.
Pass `readerRef` or `writerRef` through a function payload.

```typescript
import { TriggerAction, registerWorker } from "iii-sdk";

const iii = registerWorker(process.env.III_ENGINE_URL ?? "ws://localhost:49134", {
  workerName: "channel-pipeline",
});

iii.registerFunction("files::produce", async (input) => {
  const channel = await iii.createChannel();

  iii.trigger({
    function_id: "files::consume",
    payload: { readerRef: channel.readerRef, filename: input.filename },
    action: TriggerAction.Void(),
  });

  channel.writer.sendMessage(JSON.stringify({ contentType: "application/octet-stream" }));
  channel.writer.stream.write(Buffer.from(input.bytes, "base64"));
  channel.writer.close();

  return { streaming: true };
});

iii.registerFunction("files::consume", async (input) => {
  const reader = input.readerRef;
  const buffer = await reader.readAll();
  return { filename: input.filename, bytes: buffer.length };
});
```

## Registry Worker Capability

Use registry discovery for worker-backed capabilities. Do not invent worker-specific function IDs
or payloads before loading the worker-provided skill.

```bash
iii worker add iii-directory
iii worker sync
iii worker verify
iii worker start iii-directory
```

```typescript
await iii.trigger({
  function_id: "directory::registry::workers::list",
  payload: { search: "harness" },
});

const workerName = "harness";

await iii.trigger({
  function_id: "directory::registry::workers::info",
  payload: { name: workerName },
});

await iii.trigger({
  function_id: "directory::skills::download",
  payload: { worker: workerName, tag: "latest" },
});

const skill = await iii.trigger({
  function_id: "directory::skills::get",
  payload: { id: `${workerName}/index.md` },
});
```

## Browser Client Invocation

Use the browser SDK for UI-side functions, real-time callbacks, or invoking safe server-side
functions through an RBAC-protected listener. Keep secrets on server-side workers.

```typescript
import { TriggerAction, registerWorker } from "iii-browser-sdk";

const iii = registerWorker("wss://engine.example.com/ws?token=session-token");

iii.registerFunction("ui::notify", async (event) => {
  window.dispatchEvent(new CustomEvent("iii-notification", { detail: event }));
  return { handled: true };
});

const user = await iii.trigger({
  function_id: "users::get-profile",
  payload: { user_id: "user_123" },
});

await iii.trigger({
  function_id: "analytics::track",
  payload: { event: "profile_viewed", user_id: user.id },
  action: TriggerAction.Void(),
});
```

## Language Notes

- TypeScript/Node: use `registerWorker`, `registerFunction`, `registerTrigger`, `trigger`, `TriggerAction`, and `createChannel` from `iii-sdk`.
- Python: use `register_worker`, `register_function`, `register_trigger`, `trigger`, and sync or async channel APIs from `iii-sdk`.
- Rust: use `RegisterFunction`, `RegisterTriggerInput`, typed trigger builders, `IIITrigger`, and channel helpers such as `ChannelReader::new`, `next_binary()`, and `read_all()`.

## Related Skills

- Use `iii-functions-and-triggers` for registration details.
- Use `iii-trigger-actions` for sync vs void vs enqueue behavior.
- Use `iii-state-reactions` for reactive state triggers.
- Use `iii-channels` for stream-shaped function data.
- Use `iii-worker-catalog` before using worker-backed capabilities.
- Use `iii-error-handling` when handling SDK or engine failures.

## When to Use

- Use this skill when generating iii code and the agent needs a concrete example first.
- Use this skill when another iii skill explains a concept but the implementation shape is still unclear.
- Use this skill for core iii primitives and for registry-worker discovery flow examples.

## Boundaries

- Invoke iii functions with `trigger()`.
- Do not document or generate removed service-layer or adapter-extension APIs from older docs.
- Do not hardcode worker-specific APIs here; discover the worker and load the worker-provided skill.
- Do not use polling loops for reactive work unless the user explicitly asks for polling.
