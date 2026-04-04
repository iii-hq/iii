---
name: iii-state-reactions
description: >-
  Registers state-type triggers that automatically fire functions when key-value
  state is created, updated, or deleted within a scope. Use when building
  reactive side effects, change watchers, audit logs, cache invalidation,
  notification dispatchers, or any observer pattern where data changes should
  trigger downstream processing.
---

# State Reactions

Comparable to: Firebase onSnapshot, Convex mutations

## Key Concepts

Use the concepts below when they fit the task. Not every state reaction needs all of them.

- A **state trigger** fires whenever a value changes within a watched scope
- The handler receives `{ new_value, old_value, key, event_type }` describing the change
- **condition_function_id** gates execution — the reaction only fires if the condition returns truthy
- Multiple reactions can **independently watch** the same scope
- Reactions fire on `state::set`, `state::update`, and `state::delete` operations

## Architecture

    state::set, state::update, or state::delete
      → StateModule emits change event
        → registerTrigger type:'state' (scope match)
          → condition_function_id check (if configured)
            → registerFunction handler ({ new_value, old_value, key, event_type })

## iii Primitives Used

| Primitive                                                  | Purpose                                  |
| ---------------------------------------------------------- | ---------------------------------------- |
| `registerFunction`                                         | Define the reaction handler              |
| `registerTrigger({ type: 'state' })`                       | Watch a scope for changes                |
| `config: { scope, key, condition_function_id }`            | Scope filter and optional condition gate |
| Event payload: `{ new_value, old_value, key, event_type }` | Change details passed to the handler     |

## Reference Implementation

See [../references/state-reactions.js](../references/state-reactions.js) for the full working example — a reaction that watches a state scope and fires side effects when values change, with an optional condition gate.

Also available in **Python**: [../references/state-reactions.py](../references/state-reactions.py)

Also available in **Rust**: [../references/state-reactions.rs](../references/state-reactions.rs)

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `registerFunction(id, handler)` — define the reaction handler
- `registerTrigger({ type: 'state', config: { scope, key, condition_function_id } })` — watch for changes
- `payload.new_value` / `payload.old_value` — compare before and after
- `payload.event_type` — distinguish between set, update, and delete events
- `trigger({ function_id: 'state::set', payload })` — write derived state from the reaction
- `const logger = new Logger()` — structured logging per reaction

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Set `scope` to watch a specific domain (e.g. `orders`, `user-profiles`)
- Use `key` to narrow reactions to a single key within a scope
- Add a `condition_function_id` to filter — only react when the condition function returns truthy
- Chain reactions by writing state in one handler that triggers another reaction on a different scope

## Engine Configuration

StateModule must be enabled in iii-config.yaml for state triggers to fire. See [../references/iii-config.yaml](../references/iii-config.yaml) for the full annotated config reference.

## Pattern Boundaries

- If the task is about directly reading or writing state without reactions, prefer `iii-state-management`.
- If the task needs conditional trigger logic shared across trigger types, prefer `iii-trigger-conditions`.
- Stay with `iii-state-reactions` when the primary need is automatic side effects on state changes.

## When to Use

- Use this skill when the task is primarily about `iii-state-reactions` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
