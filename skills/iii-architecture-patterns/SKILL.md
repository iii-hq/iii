---
name: iii-architecture-patterns
description: >-
  Use when composing iii primitives into backend architectures: durable workflows,
  reactive backends, observer side effects, agentic pipelines, event-driven CQRS, effect
  pipelines, and event-transform-action automation.
---

# Architecture Patterns

Use this skill when the task is larger than one function or observer. Pick the pattern from the
requirement, then implement it with workers, functions, observers, state, queues, and streams.

## Pattern Map

| Requirement | Pattern | iii shape |
| --- | --- | --- |
| Sequential work with retries, DLQ, step tracking | Durable workflow | Functions chained through named queues, progress in state |
| Keep views, metrics, cache, or clients in sync | Reactive backend | Observers of `state::on-changed` that push to streams |
| React to another function's calls, results, or failures without changing it | Observer side effect | `iii.observe({ function_id, on, filters }, target)` (`iii-observe`) |
| Specialized AI agents hand work to each other | Agentic backend | One function per agent, queue handoffs, shared state |
| Commands produce events and projections update independently | Event-driven CQRS | Command functions that return the event, event log in state, projections that observe the commands |
| Pure, traceable composition | Effect pipeline | Small functions composed synchronously with `call()` |
| Webhook/cron automation chains | Low-code automation | An observer of `http::on-request` or `cron::on-tick`, then transform and action functions chained by enqueue |

## Durable Workflow

Use named queues when steps need different retry, FIFO, or concurrency policy.

### TypeScript

```typescript
import { registerWorker, CallAction } from "iii-sdk";

const iii = registerWorker("ws://localhost:49134", { workerName: "order-workflow" });

async function track(orderId: string, step: string, status: string) {
  await iii.call({
    function_id: "state::update",
    payload: { scope: "orders", key: orderId, ops: [{ type: "merge", path: ["steps"], value: { [step]: status } }] },
  });
}

iii.registerFunction("orders::validate", async (order) => {
  await track(order.id, "validate", "done");
  return iii.call({
    function_id: "orders::charge",
    payload: order,
    action: CallAction.Enqueue({ queue: "order-payment" }),
  });
});

iii.registerFunction("orders::charge", async (order) => {
  await track(order.id, "payment", "done");
  return iii.call({
    function_id: "orders::ship",
    payload: order,
    action: CallAction.Enqueue({ queue: "order-ship" }),
  });
});
```

### Python

```python
from iii import register_worker

iii = register_worker("ws://localhost:49134")

def track(order_id, step, status):
    iii.call({
        "function_id": "state::update",
        "payload": {
            "scope": "orders",
            "key": order_id,
            "ops": [{"type": "merge", "path": ["steps"], "value": {step: status}}],
        },
    })

def validate(order):
    track(order["id"], "validate", "done")
    return iii.call({
        "function_id": "orders::charge",
        "payload": order,
        "action": {"type": "enqueue", "queue": "order-payment"},
    })

iii.register_function("orders::validate", validate)
```

### Rust

```rust
use iii_sdk::CallAction;
use iii_sdk::protocol::CallRequest;
use serde_json::json;

async fn enqueue_charge(iii: iii_sdk::IIIClient, order: serde_json::Value) -> Result<serde_json::Value, iii_sdk::Error> {
    iii.call(CallRequest {
        function_id: "state::update".into(),
        payload: json!({
            "scope": "orders",
            "key": order["id"],
            "ops": [{ "type": "merge", "path": ["steps"], "value": { "validate": "done" } }]
        }),
        action: None,
        timeout_ms: None,
    }).await?;

    iii.call(CallRequest {
        function_id: "orders::charge".into(),
        payload: order,
        action: Some(CallAction::Enqueue { queue: "order-payment".into() }),
        timeout_ms: None,
    }).await
}
```

## Reactive Backend

Observe `state::on-changed` when the requirement says "after create/update, do this", "avoid
polling", or "push live updates".

```typescript
iii.registerFunction("todos::on-change", async (event) => {
  await iii.call({
    function_id: "stream::send",
    payload: { stream_name: "todos-live", group_id: "default", data: event.new_value },
    action: CallAction.Void(),
  });
});

// each change in the "todos" state scope
iii.observe(
  { function_id: "state::on-changed", deliver: "input", filters: { "input.scope": { $eq: "todos" } } },
  "todos::on-change",
);
```

## Observer Side Effects

Use an observer when "after X runs (or fails), also do Y" and X should not know about Y: audit
logs, alerts, projections, cache invalidation. Every function is observable, and an observer
failure never affects the observed call.

```typescript
import type { Observed } from "iii-sdk";

iii.registerFunction("audit::order-created", async (event: Observed) => {
  await iii.call({
    function_id: "state::set",
    payload: { scope: "audit", key: event.invocation_id, value: { order: event.input, by: event.caller?.worker_name } },
  });
  return null;
});

// every successful orders::create, except the replay tool's own calls
iii.observe(
  { function_id: "orders::create", filters: { "caller.worker_name": { $ne: "order-replayer" } } },
  "audit::order-created",
  { max_in_flight: 8 },
);
```

```python
def notify(event):  # the Observed envelope: input, error, caller, ...
    print("charge failed", event["input"], event["error"]["code"])

iii.register_function("alerts::payment-failed", notify)
iii.observe(
    {"function_id": "payments::charge", "on": "error", "filters": {"input.amount": {"$gte": 500}}},
    "alerts::payment-failed",
)
```

Activations are at most once and fire-and-forget. When the side effect must complete with retries,
have the target enqueue the work (`CallAction.Enqueue`). Filter on `caller` or
`caller.parent` to stop echo loops; the engine also drops activations deeper than
`observers.max_observe_depth` (8 by default).

## Agentic Backend

Model each agent as a function with one responsibility. Store shared context in state and hand off
work through named queues.

```typescript
iii.registerFunction("agents::researcher", async (task) => {
  await iii.call({
    function_id: "state::set",
    payload: { scope: "research", key: task.id, value: { findings: [] } },
  });
  return iii.call({
    function_id: "agents::critic",
    payload: task,
    action: CallAction.Enqueue({ queue: "agent-tasks" }),
  });
});
```

## Event-Driven CQRS

Commands validate and return domain events. Projections observe the commands independently and
write query-optimized state.

```typescript
iii.registerFunction("cmd::add-inventory-item", async (input) => {
  const event = { type: "inventory.item-added", itemId: input.itemId, quantity: input.quantity };
  await iii.call({
    function_id: "state::set",
    payload: { scope: "inventory-events", key: `${Date.now()}-${input.itemId}`, value: event },
  });
  return event; // what observers observe
});

// each projection receives the event every successful command returned
iii.observe({ function_id: "cmd::add-inventory-item", deliver: "output" }, "proj::inventory-list");
iii.observe({ function_id: "cmd::add-inventory-item", deliver: "output" }, "proj::inventory-totals");
```

When a projection must process every event with retries, publish the event to a queue topic
(`iii.call({ function_id: "iii::durable::publish", payload: { topic, data: event } })`) and have
each projection observe `queue::on-message` with `{ topic }`: returning acks a message, an error
retries it, then moves it to the topic's DLQ.

## Selection Rules

- Use sync composition for short effect pipelines where the caller needs the final result.
- Use enqueue for unreliable, slow, or must-complete steps.
- Use observers for independent fan-out where each consumer can tolerate at-most-once delivery;
  use a queue topic (`queue::on-message`) when each consumer needs retries.
- Observe `state::on-changed` for derived views and side effects that should run automatically
  after writes.
- Use observers for side effects of a specific function's calls, results, or failures, when the
  caller should not know about them and at-most-once delivery is acceptable.
- Observe `cron::on-tick` for scheduled maintenance and periodic starts.
- Keep each function small enough to test independently.

## When to Use

- Use this skill for complete backend patterns: workflows, agentic systems, reactive apps, CQRS,
  effect pipelines, and automation chains.
- Use this when the request describes product behavior rather than a single SDK API call.

## Boundaries

- For built-in source args, function registration syntax, calls, channels, and HTTP-invoked
  functions, use `iii-core-primitives`.
- For observer options, filters, and writing event sources, use `iii-observe`.
- For engine ports, adapters, engine-owned workers, and RBAC (`rbac-proxy`), use `iii-engine-config`;
  queue retry and FIFO policy live with the queue worker's docs.
- For package-specific SDK syntax, use `iii-sdk-reference`.
- Worker-backed capability details live with the worker docs, not as top-level iii skills.
