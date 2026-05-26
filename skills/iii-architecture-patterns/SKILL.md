---
name: iii-architecture-patterns
description: >-
  Use when composing iii primitives into backend architectures: durable workflows,
  reactive backends, agentic pipelines, event-driven CQRS, effect pipelines, and
  trigger-transform-action automation.
---

# Architecture Patterns

Use this skill when the task is larger than one function or trigger. Pick the pattern from the
requirement, then implement it with `Function`, `Trigger`, `Worker`, state, queues, streams, and
pub/sub.

## Pattern Map

| Requirement | Pattern | iii shape |
| --- | --- | --- |
| Sequential work with retries, DLQ, step tracking | Durable workflow | Functions chained through named queues, progress in state |
| Keep views, metrics, cache, or clients in sync | Reactive backend | State triggers plus stream/pubsub side effects |
| Specialized AI agents hand work to each other | Agentic backend | One function per agent, queue handoffs, shared state |
| Commands publish events and projections update independently | Event-driven CQRS | Command functions, event log in state, subscribe triggers |
| Pure, traceable composition | Effect pipeline | Small functions composed synchronously with `trigger()` |
| Webhook/cron automation chains | Low-code automation | Trigger, transform, action nodes chained by enqueue |

## Durable Workflow

Use named queues when steps need different retry, FIFO, or concurrency policy.

### TypeScript

```typescript
import { registerWorker, TriggerAction } from "iii-sdk";

const iii = registerWorker("ws://localhost:49134", { workerName: "order-workflow" });

async function track(orderId: string, step: string, status: string) {
  await iii.trigger({
    function_id: "state::update",
    payload: { scope: "orders", key: orderId, ops: [{ op: "set", path: `/steps/${step}`, value: status }] },
  });
}

iii.registerFunction("orders::validate", async (order) => {
  await track(order.id, "validate", "done");
  return iii.trigger({
    function_id: "orders::charge",
    payload: order,
    action: TriggerAction.Enqueue({ queue: "order-payment" }),
  });
});

iii.registerFunction("orders::charge", async (order) => {
  await track(order.id, "payment", "done");
  return iii.trigger({
    function_id: "orders::ship",
    payload: order,
    action: TriggerAction.Enqueue({ queue: "order-ship" }),
  });
});
```

### Python

```python
from iii import register_worker

iii = register_worker("ws://localhost:49134")

def track(order_id, step, status):
    iii.trigger({
        "function_id": "state::update",
        "payload": {
            "scope": "orders",
            "key": order_id,
            "ops": [{"op": "set", "path": f"/steps/{step}", "value": status}],
        },
    })

def validate(order):
    track(order["id"], "validate", "done")
    return iii.trigger({
        "function_id": "orders::charge",
        "payload": order,
        "action": {"type": "enqueue", "queue": "order-payment"},
    })

iii.register_function("orders::validate", validate)
```

### Rust

```rust
use iii_sdk::{TriggerAction, TriggerRequest};
use serde_json::json;

async fn enqueue_charge(iii: iii_sdk::III, order: serde_json::Value) -> Result<serde_json::Value, iii_sdk::IIIError> {
    iii.trigger(TriggerRequest {
        function_id: "state::update".into(),
        payload: json!({
            "scope": "orders",
            "key": order["id"],
            "ops": [{ "op": "set", "path": "/steps/validate", "value": "done" }]
        }),
        action: None,
        timeout_ms: None,
    }).await?;

    iii.trigger(TriggerRequest {
        function_id: "orders::charge".into(),
        payload: order,
        action: Some(TriggerAction::Enqueue { queue: "order-payment".into() }),
        timeout_ms: None,
    }).await
}
```

## Reactive Backend

Use state triggers when the requirement says "after create/update, do this", "avoid polling", or
"push live updates".

```typescript
iii.registerFunction("todos::on-change", async (event) => {
  await iii.trigger({
    function_id: "stream::send",
    payload: { stream_name: "todos-live", group_id: "default", data: event.new_value },
    action: TriggerAction.Void(),
  });
});

iii.registerTrigger({
  type: "state",
  function_id: "todos::on-change",
  config: { scope: "todos" },
});
```

## Agentic Backend

Model each agent as a function with one responsibility. Store shared context in state and hand off
work through named queues.

```typescript
iii.registerFunction("agents::researcher", async (task) => {
  await iii.trigger({
    function_id: "state::set",
    payload: { scope: "research", key: task.id, value: { findings: [] } },
  });
  return iii.trigger({
    function_id: "agents::critic",
    payload: task,
    action: TriggerAction.Enqueue({ queue: "agent-tasks" }),
  });
});
```

## Event-Driven CQRS

Commands validate and publish domain events. Projections subscribe independently and write
query-optimized state.

```typescript
iii.registerFunction("cmd::add-inventory-item", async (input) => {
  const event = { type: "inventory.item-added", itemId: input.itemId, quantity: input.quantity };
  await iii.trigger({
    function_id: "state::set",
    payload: { scope: "inventory-events", key: `${Date.now()}-${input.itemId}`, value: event },
  });
  await iii.trigger({ function_id: "publish", payload: { topic: event.type, data: event } });
  return { accepted: true };
});

iii.registerTrigger({
  type: "subscribe",
  function_id: "proj::inventory-list",
  config: { topic: "inventory.item-added" },
});
```

## Selection Rules

- Use sync composition for short effect pipelines where the caller needs the final result.
- Use enqueue for unreliable, slow, or must-complete steps.
- Use pub/sub for independent fan-out where each subscriber can tolerate event-style delivery.
- Use state triggers for derived views and side effects that should run automatically after writes.
- Use cron triggers for scheduled maintenance and periodic starts.
- Keep each function small enough to test independently.

## When to Use

- Use this skill for complete backend patterns: workflows, agentic systems, reactive apps, CQRS,
  effect pipelines, and automation chains.
- Use this when the request describes product behavior rather than a single SDK API call.

## Boundaries

- For exact trigger config, function registration syntax, custom triggers, channels, and
  HTTP-invoked functions, use `iii-core-primitives`.
- For queue retry, FIFO, adapter, port, and worker-manager config, use `iii-engine-config`.
- For package-specific SDK syntax, use `iii-sdk-reference`.
- Worker-backed capability details live with the worker docs, not as top-level iii skills.
