---
name: iii-low-code-automation
description: >-
  Builds trigger-transform-action automation chains on the iii engine. Use when
  building Zapier/n8n-style automations, webhook-to-action pipelines, or simple
  event-driven chains where each node is a small registered function chained via
  named queues.
---

# Low-Code Automation Chains

Comparable to: n8n, Zapier, LangFlow

## Key Concepts

Use the concepts below when they fit the task. Not every automation needs all of them.

- Each "node" in the automation is a small registered function with a single job
- Nodes chain via **named queues** using `TriggerAction.Enqueue` — easy to add, remove, or reorder steps
- **HTTP triggers** receive external webhooks (form submissions, payment events)
- **Cron triggers** start scheduled automations (daily digests, periodic syncs)
- **PubSub** broadcasts completion events for downstream listeners

## Architecture

```text
Automation 1: Form → Enrich → Store → Notify
  HTTP webhook → auto::enrich-lead → auto::store-lead → auto::notify-slack

Automation 2: Cron → Fetch → Transform → Store
  Cron (daily) → auto::fetch-rss → auto::transform-articles → auto::store-articles

Automation 3: Payment webhook → Validate → Update → Notify
  HTTP webhook → auto::validate-payment → auto::update-order → publish(payment.processed)
```

## iii Primitives Used

| Primitive                                                    | Purpose                                  |
| ------------------------------------------------------------ | ---------------------------------------- |
| `registerWorker`                                             | Initialize the worker and connect to iii |
| `registerFunction`                                           | Define each automation node              |
| `trigger({ ..., action: TriggerAction.Enqueue({ queue }) })` | Chain nodes via named queues             |
| `trigger({ function_id: 'state::set', payload })`            | Persist data between nodes               |
| `trigger({ ..., action: TriggerAction.Void() })`             | Fire-and-forget notifications            |
| `registerTrigger({ type: 'http' })`                          | Webhook entry points                     |
| `registerTrigger({ type: 'cron' })`                          | Scheduled automations                    |

## Reference Implementation

See [../references/low-code-automation.js](../references/low-code-automation.js) for the full working example — three automation chains:
form-to-Slack notification, RSS feed aggregation, and payment webhook processing.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `trigger({ function_id, payload, action: TriggerAction.Enqueue({ queue: 'automation' }) })` — node chaining
- Each node as its own `registerFunction` with `auto::` prefix IDs
- Small, focused functions that do one thing (enrich, validate, store, notify)
- `trigger({ function_id: 'state::set', payload: { scope, key, value } })` — persist between nodes
- `const logger = new Logger()` — structured logging per node

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Add new automation chains by registering HTTP/cron triggers and chaining functions
- Each node should be independently testable — accept input, produce output
- Use separate queue names when different chains need different retry/concurrency settings
- For unreliable external services, wrap calls in try/catch and handle failures explicitly
- Keep node functions small — offload complex logic to dedicated functions

## Pattern Boundaries

- If the task requires durable multi-step workflows with saga compensation and step tracking, prefer `iii-workflow-orchestration`.
- If the task involves multiple AI agents handing off work, prefer `iii-agentic-backend`.
- Stay with `iii-low-code-automation` when the primary concern is simple trigger-transform-action chains with minimal orchestration overhead.

## When to Use

- Use this skill when the task is primarily about `iii-low-code-automation` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
