---
name: iii-reactive-backend
description: >-
  Builds reactive real-time backends where functions react automatically to
  state changes and stream events. Use when requirements say "after create/update
  do this", "keep views/metrics/cache in sync", "notify clients when data
  changes", "push live updates", or "avoid polling"; combine state triggers,
  stream updates, pub/sub, and queued work instead of imperative follow-up calls.
---

# Reactive Backend

Comparable to: Convex, Firebase, Supabase, Appwrite

## Key Concepts

Use the concepts below when they fit the task. Not every reactive backend needs every trigger or realtime surface shown here.

- State is the "database" — CRUD via `state::set`, `state::get`, `state::update`, `state::delete`, `state::list`
- **State triggers** fire automatically when any value in a scope changes
- Side effects (notifications, metrics, stream pushes) are wired reactively, not imperatively
- **Streams** deliver real-time updates to connected clients
- Prefer state triggers, stream triggers, queue subscribers, and pub/sub over polling loops for iii-native data flow

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

## Code Example

```typescript
import { registerWorker, TriggerAction } from "iii-sdk";

const iii = registerWorker("ws://localhost:49134", { workerName: "reactive-todos" });

iii.registerFunction("todos::create", async (input) => {
  const todo = { id: crypto.randomUUID(), text: input.text, done: false };
  await iii.trigger({
    function_id: "state::set",
    payload: { scope: "todos", key: todo.id, value: todo },
  });
  return todo;
});

iii.registerFunction("todos::on-change", async (event) => {
  await iii.trigger({
    function_id: "stream::send",
    payload: { stream_name: "todos-live", group_id: "default", data: event.new_value },
    action: TriggerAction.Void(),
  });
  return { pushed: true };
});

iii.registerTrigger({
  type: "state",
  function_id: "todos::on-change",
  config: { scope: "todos" },
});
```

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
- Use this skill even when the request does not say "reactive" if the backend should respond automatically to changes or push updates to clients.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
