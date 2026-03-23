---
name: effect-system
description: >-
  Builds composable, pipeable function chains on the iii engine. Use when
  building functional pipelines, effect systems, or typed composition layers
  where each step is a pure function with distributed tracing and retry.
---

# Effect Systems & Typed Functional Infrastructure

Comparable to: Effect-TS

## Key Concepts

Use the concepts below when they fit the task. Not every effect pipeline needs all of them.

- Each effect is a registered function with a single responsibility (parse, enrich, persist, notify)
- Effects compose by calling one function from another via `trigger`
- The entire pipeline is traceable end-to-end through OpenTelemetry
- Errors propagate naturally — a failing effect stops the chain
- An HTTP trigger provides the entry point; effects chain from there

## Architecture

```
HTTP request
  → fx::parse-user-input (validate + normalize)
    → fx::enrich (add metadata, lookup external data)
      → fx::persist (write to state)
        → fx::notify (fire-and-forget side effect)
  ← composed result returned to caller
```

## iii Primitives Used

| Primitive                                                | Purpose                                  |
| -------------------------------------------------------- | ---------------------------------------- |
| `registerWorker`                                         | Initialize the worker and connect to iii |
| `registerFunction`                                       | Define each effect                       |
| `trigger({ function_id, payload })`                      | Compose effects synchronously            |
| `trigger({ ..., action: TriggerAction.Void() })`         | Fire-and-forget side effects             |
| `trigger({ function_id: 'state::set/get', payload })`    | Persist data between effects             |
| `registerTrigger({ type: 'http' })`                      | Entry point                              |

## Reference Implementation

See [../references/effect-system.js](../references/effect-system.js) for the full working example — a user signup pipeline
where input is parsed, enriched with external data, persisted to state, and a welcome notification is fired.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `trigger({ function_id, payload })` — synchronous composition (effect A calls effect B)
- Each effect as its own `registerFunction` with `fx::` prefix IDs
- Error throwing for validation failures (errors propagate up the chain)
- `trigger({ ..., action: TriggerAction.Void() })` — fire-and-forget for non-critical side effects
- `const logger = new Logger()` — structured logging per effect

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Replace simulated logic with real work (API calls, database queries, ML inference)
- Add new effects by registering functions and calling them via `trigger`
- For unreliable steps, use `TriggerAction.Enqueue({ queue })` instead of synchronous `trigger`
- Keep effects pure where possible — accept input, return output, no hidden side effects
- Function IDs should be domain-prefixed (e.g. `fx::validate-email`, `fx::geocode-address`)

## Pattern Boundaries

- If a request is about durable multi-step workflows with retries and DLQ handling, prefer `workflow-orchestration`.
- If the task involves multiple independent agents handing off work, prefer `agentic-backend`.
- Stay with `effect-system` when the primary concern is composable, traceable function pipelines with synchronous chaining.
