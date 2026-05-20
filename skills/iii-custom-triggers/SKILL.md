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

## Reference Implementation

See [../references/custom-triggers.js](../references/custom-triggers.js) for the full working example — a custom trigger type that listens for external events and routes them to registered functions.

Also available in **Python**: [../references/custom-triggers.py](../references/custom-triggers.py)

Also available in **Rust**: [../references/custom-triggers.rs](../references/custom-triggers.rs)

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

- If the task uses built-in HTTP routes, prefer `iii-http-endpoints`.
- If the task uses built-in cron schedules, prefer `iii-cron-scheduling`.
- If the task uses built-in queue triggers, prefer `iii-queue-processing`.
- Stay with `iii-custom-triggers` when iii has no built-in trigger type for the event source.

## When to Use

- Use this skill when the task is primarily about `iii-custom-triggers` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
