---
name: custom-triggers
description: >-
  Builds custom trigger types for events iii does not handle natively. Use when
  integrating webhooks, file watchers, IoT devices, database CDC, or any
  external event source.
---

# Custom Triggers

Comparable to: Custom event adapters, webhook receivers

## Key Concepts

Use the concepts below when they fit the task. Not every custom trigger needs all of them.

- **registerTriggerType(id, handler)** defines a new trigger type with `registerTrigger` and `unregisterTrigger` callbacks
- The handler receives a **TriggerConfig** containing `id`, `function_id`, and `config`
- When the external event fires, call `iii.trigger(function_id, event)` to invoke the registered function
- **unregisterTriggerType** cleans up when the trigger type is no longer needed
- Do not reuse built-in trigger type names: `http`, `cron`, `queue`, `state`, `stream`, `subscribe`

## Architecture

    External event source (webhook, file watcher, IoT, CDC, etc.)
      → Custom trigger handler (registerTriggerType)
        → iii.trigger(function_id, event)
          → Registered function processes the event

## iii Primitives Used

| Primitive                                    | Purpose                                            |
| -------------------------------------------- | -------------------------------------------------- |
| `registerTriggerType(id, handler)`           | Define a new trigger type with lifecycle hooks     |
| `unregisterTriggerType(id)`                  | Clean up a custom trigger type                     |
| `TriggerConfig: { id, function_id, config }` | Configuration passed to the trigger handler        |
| `iii.trigger(function_id, event)`            | Fire the registered function when the event occurs |

## Reference Implementation

See [../references/custom-triggers.js](../references/custom-triggers.js) for the full working example — a custom trigger type that listens for external events and routes them to registered functions.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `registerTriggerType(id, { registerTrigger, unregisterTrigger })` — define the custom trigger
- `registerTrigger(config)` — called by iii when a function subscribes to this trigger type
- `unregisterTrigger(config)` — called by iii when a function unsubscribes
- `iii.trigger(config.function_id, eventPayload)` — fire the target function
- Cleanup logic in `unregisterTrigger` (close connections, remove listeners, clear intervals)
- `const logger = new Logger()` — structured logging

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Choose a unique trigger type name that describes your event source (e.g. `file-watcher`, `mqtt`, `db-cdc`)
- In `registerTrigger`, start the listener (open socket, poll endpoint, subscribe to topic)
- In `unregisterTrigger`, tear down the listener to avoid resource leaks
- Store active listeners in a map keyed by `config.id` for clean unregistration
- Pass relevant event data in the payload when calling `iii.trigger(function_id, event)`

## Pattern Boundaries

- If the task uses built-in HTTP routes, prefer `http-endpoints`.
- If the task uses built-in cron schedules, prefer `cron-scheduling`.
- If the task uses built-in queue triggers, prefer `queue-processing`.
- Stay with `custom-triggers` when iii has no built-in trigger type for the event source.

## When to Use

- Use this skill when the task is primarily about `custom-triggers` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
