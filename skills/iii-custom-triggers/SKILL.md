---
name: iii-custom-triggers
description: >-
  Builds custom trigger types for events iii does not handle natively. Use when
  integrating webhooks, file watchers, IoT devices, database CDC, or any
  external event source.
---

# Custom Triggers

Comparable to: Custom event adapters, webhook receivers

## Key Concepts

Use the concepts below when they fit the task. Not every custom trigger needs all of them.

- `registerTriggerType({ id, description }, handler)` defines a new trigger type with `registerTrigger` and `unregisterTrigger` callbacks
- The handler receives a **TriggerConfig** containing `id`, `function_id`, `config`, and optional `metadata`
- When the external event fires, call `iii.trigger({ function_id, payload: event })` to invoke the registered function
- **unregisterTriggerType** cleans up when the trigger type is no longer needed
- Do not reuse built-in trigger type names: `http`, `cron`, `durable:subscriber`, `state`, `stream`, `subscribe`

## Architecture

    External event source (webhook, file watcher, IoT, CDC, etc.)
      → Custom trigger handler (registerTriggerType)
        → iii.trigger({ function_id, payload: event })
          → Registered function processes the event

## iii Primitives Used

| Primitive                                    | Purpose                                            |
| -------------------------------------------- | -------------------------------------------------- |
| `registerTriggerType({ id, description }, handler)` | Define a new trigger type with lifecycle hooks |
| `unregisterTriggerType(id)`                  | Clean up a custom trigger type                     |
| `TriggerConfig: { id, function_id, config, metadata? }` | Configuration passed to the trigger handler |
| `iii.trigger({ function_id, payload: event })`| Fire the registered function when the event occurs |

## Code Examples

TypeScript:

```typescript
import { registerWorker } from "iii-sdk";
import { EventEmitter } from "node:events";

const iii = registerWorker("ws://localhost:49134", { workerName: "webhook-source" });
const source = new EventEmitter();
const bindings = new Map();

iii.registerTriggerType({ id: "external-webhook", description: "External webhook events" }, {
  registerTrigger(config) {
    const listener = (event) => iii.trigger({ function_id: config.function_id, payload: event });
    bindings.set(config.id, listener);
    source.on(config.config.event, listener);
  },
  unregisterTrigger(config) {
    const listener = bindings.get(config.id);
    if (listener) source.off(config.config.event, listener);
    bindings.delete(config.id);
  },
});
```

Python:

```python
from iii import TriggerHandler, register_worker

iii = register_worker("ws://localhost:49134")

class ExternalWebhook(TriggerHandler):
    def __init__(self):
        self.bindings = {}

    def register_trigger(self, config):
        self.bindings[config.id] = config.function_id

    def unregister_trigger(self, config):
        self.bindings.pop(config.id, None)

    def emit(self, name, payload):
        for function_id in self.bindings.values():
            iii.trigger({"function_id": function_id, "payload": {"event": name, "data": payload}})

handler = ExternalWebhook()
iii.register_trigger_type({"id": "external-webhook", "description": "External webhook events"}, handler)
```

Rust:

```rust
use iii_sdk::{register_worker, InitOptions, RegisterTriggerType, TriggerConfig, TriggerHandler};
use async_trait::async_trait;

struct ExternalWebhook;

#[async_trait]
impl TriggerHandler for ExternalWebhook {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), iii_sdk::IIIError> {
        Ok(())
    }

    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), iii_sdk::IIIError> {
        Ok(())
    }
}

let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default());
iii.register_trigger_type(RegisterTriggerType::new(
    "external-webhook",
    "External webhook events",
    ExternalWebhook,
))?;
```

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- Node: `iii.registerTriggerType({ id, description }, { registerTrigger, unregisterTrigger })`
- Python: `iii.register_trigger_type({ "id": id, "description": description }, TriggerHandlerSubclass())`
- Rust: `iii.register_trigger_type(RegisterTriggerType::new(id, description, handler))`
- `registerTrigger(config)` / `register_trigger(config)` is invoked by the engine when a trigger instance is registered
- `unregisterTrigger(config)` / `unregister_trigger(config)` is invoked when that trigger instance is removed
- `iii.trigger({ function_id: config.function_id, payload: eventPayload })` — fire the target function
- Cleanup logic in `unregisterTrigger` (close connections, remove listeners, clear intervals)
- `const logger = new Logger()` — structured logging

## Authoring Guidance

- Store trigger bindings in a map keyed by `config.id`.
- Keep long-lived resources per trigger binding: sockets, timers, webhook routes, subscriptions, or file watchers.
- Put subscription/listener setup in `registerTrigger`, not in module top-level code.
- Put cleanup in `unregisterTrigger` so deleted triggers do not leak timers or connections.
- Put static event-source settings in trigger `config`.
- Put event-specific data in the call payload sent to `iii.trigger()`.
- If using `condition_function_id`, let the trigger dispatch path evaluate the condition before invoking the target function.
- Add `trigger_request_format` and `call_request_format` schemas where the SDK supports them, so discovery and generated skills know the config and handler payload shapes.

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Choose a unique trigger type name that describes your event source (e.g. `file-watcher`, `mqtt`, `db-cdc`)
- In `registerTrigger`, start the listener (open socket, receive webhook, subscribe to topic, attach watcher, or consume CDC)
- Use polling only as a fallback when the external source has no webhook, subscription, stream, or CDC mechanism.
- In `unregisterTrigger`, tear down the listener to avoid resource leaks
- Store active listeners in a map keyed by `config.id` for clean unregistration
- Pass relevant event data in the payload when calling `iii.trigger({ function_id, payload: event })`

## Pattern Boundaries

- If the task uses built-in trigger config or handler payloads, prefer `iii-trigger-schemas`.
- If the task uses built-in worker-backed HTTP, cron, queue, pubsub, state, or stream behavior, use the matching worker docs under `engine/src/workers/**/skills`.
- Stay with `iii-custom-triggers` when iii has no built-in trigger type for the event source.

## When to Use

- Use this skill when the task is primarily about `iii-custom-triggers` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
