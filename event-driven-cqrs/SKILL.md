---
name: event-driven-cqrs
description: >-
  Implements CQRS with event sourcing on the iii engine. Use when building
  command/query separation, event-sourced systems, or fan-out architectures
  where commands publish domain events and multiple read model projections
  subscribe independently.
---

# Event-Driven CQRS & Event Sourcing

Comparable to: Kafka, RabbitMQ, CQRS/Event Sourcing systems

## Key Concepts

Use the concepts below when they fit the task. Not every CQRS system needs all of them.

- **Write side**: Commands validate input and publish domain events via pubsub
- **Read side**: Multiple projections subscribe to events independently, building query-optimized views in state
- **Event log**: Events are appended to state as an ordered log (event sourcing)
- **PubSub** handles fan-out — one event reaches all projections and downstream consumers
- **HTTP triggers** expose both command endpoints (POST) and query endpoints (GET)

## Architecture

```text
HTTP POST /inventory (command)
  → cmd::add-inventory-item → validate → append event to state
    → publish('inventory.item-added')
      ↓ (fan-out via subscribe triggers)
      → proj::inventory-list (updates queryable list view)
      → proj::inventory-stats (updates aggregate counters)
      → notify::inventory-alert (sends low-stock alerts)

HTTP GET /inventory (query)
  → query::list-inventory → reads from projection state
```

## iii Primitives Used

| Primitive                                                   | Purpose                                   |
| ----------------------------------------------------------- | ----------------------------------------- |
| `registerWorker`                                            | Initialize the worker and connect to iii  |
| `registerFunction`                                          | Define commands, projections, and queries |
| `trigger({ function_id: 'state::set/get/list', payload })`  | Event log and projection state            |
| `trigger({ function_id: 'publish', payload })`              | Publish domain events                     |
| `registerTrigger({ type: 'subscribe', config: { topic } })` | Subscribe projections to events           |
| `registerTrigger({ type: 'http' })`                         | Command and query endpoints               |
| `trigger({ ..., action: TriggerAction.Void() })`            | Fire-and-forget notifications             |

## Reference Implementation

See [../references/event-driven-cqrs.js](../references/event-driven-cqrs.js) for the full working example — an inventory management system
with commands that publish domain events and multiple projections building query-optimized views.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `trigger({ function_id: 'state::set', payload: { scope: 'events', key, value } })` — event log append
- `trigger({ function_id: 'publish', payload: { topic, data } })` — domain event publishing
- `registerTrigger({ type: 'subscribe', function_id, config: { topic } })` — projection subscriptions
- Command functions with `cmd::` prefix, projection functions with `proj::` prefix, query functions with `query::` prefix
- Multiple projections subscribing to the same topic independently
- `const logger = new Logger()` — structured logging per command/projection

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Add new projections by registering subscribe triggers on existing event topics
- Use separate state scopes for each projection (e.g. `inventory-list`, `inventory-stats`)
- Commands should validate before publishing — reject invalid commands early
- For critical event processing, use `TriggerAction.Enqueue({ queue })` instead of pubsub for guaranteed delivery
- Event IDs should be unique and monotonic for ordering (e.g. `evt-${Date.now()}-${counter}`)

## Pattern Boundaries

- If the task is about simple CRUD with reactive side effects, prefer `reactive-backend`.
- If the task needs durable multi-step pipelines with retries, prefer `workflow-orchestration`.
- Stay with `event-driven-cqrs` when command/query separation, event sourcing, and independent projections are the primary concerns.

## When to Use

- Use this skill when the task is primarily about `event-driven-cqrs` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
