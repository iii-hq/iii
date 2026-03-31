---
name: iii-reactive-backend
description: >-
  Builds reactive real-time backends on the iii engine. Use when building
  event-driven apps where state changes automatically trigger side effects,
  clients receive live updates via streams or websockets, or you need a
  real-time database layer with pub/sub and CRUD endpoints.
---

# Reactive Backend

Comparable to: Convex, Firebase, Supabase, Appwrite

## Key Concepts

Use the concepts below when they fit the task. Not every reactive backend needs every trigger or realtime surface shown here.

- State is the "database" — CRUD via `state::set`, `state::get`, `state::update`, `state::delete`, `state::list`
- **State triggers** fire automatically when any value in a scope changes
- Side effects (notifications, metrics, stream pushes) are wired reactively, not imperatively
- **Streams** deliver real-time updates to connected clients

## Architecture

```text
HTTP CRUD endpoints
  → `state::set`, `state::update`, `state::delete` (writes to 'todos' scope)
    ↓ (automatic state triggers)
    → on-change → stream::send (push to clients)
    → update-metrics → state::update (aggregate counters)

HTTP GET /metrics → reads from 'todo-metrics' scope
WebSocket clients ← stream 'todos-live'
```

## iii Primitives Used

| Primitive                                               | Purpose                                  |
| ------------------------------------------------------- | ---------------------------------------- |
| `registerWorker`                                        | Initialize the worker and connect to iii |
| `registerFunction`                                      | CRUD handlers and reactive side effects  |
| `trigger({ function_id: 'state::...', payload })`       | Database layer                           |
| `registerTrigger({ type: 'state', config: { scope } })` | React to any change in a scope           |
| `trigger({ ..., action: TriggerAction.Void() })`        | Fire-and-forget stream push to clients   |
| `registerTrigger({ type: 'http' })`                     | REST endpoints                           |

## Reference Implementation

See [../references/reactive-backend.js](../references/reactive-backend.js) for the full working example — a real-time todo app with
CRUD endpoints, automatic change broadcasting via streams, and reactive aggregate metrics.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- trigger `state::set`, `state::get` — CRUD via state module
- `registerTrigger({ type: 'state', function_id, config: { scope } })` — reactive side effects on state change
- Event argument destructuring in reactive handlers: `async (event) => { const { new_value, old_value, key } = event }`
- `trigger({ function_id: 'stream::send', payload, action: TriggerAction.Void() })` — push live updates to clients
- `const logger = new Logger()` — structured logging inside handlers

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- State triggers fire on **any** change in the scope — use the `event` argument (`new_value`, `old_value`, `key`) to determine what changed
- Multiple functions can react to the same scope independently (on-change and update-metrics both watch `todos`)
- Stream clients connect via `ws://host:port/stream/{stream_name}/{group_id}`
- Keep reactive functions fast — offload heavy work to queues if needed

## Pattern Boundaries

- If the request focuses on registering external/legacy HTTP endpoints via `registerFunction` (especially with endpoint lists like `{ path, id }` plus iteration), prefer `iii-http-invoked-functions`.
- Stay with `iii-reactive-backend` when state scopes, state triggers, and live stream updates are the core requirement.

## When to Use

- Use this skill when the task is primarily about `iii-reactive-backend` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
