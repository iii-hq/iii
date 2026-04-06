---
name: iii-agentic-backend
description: >-
  Creates and orchestrates multi-agent pipelines on the iii engine. Use when
  building AI agent collaboration, agent orchestration, research/review/synthesis
  chains, or any system where specialized agents hand off work through queues
  and shared state.
---

# Agentic Backend

Comparable to: LangGraph, CrewAI, AutoGen, Letta

## Key Concepts

Use the concepts below when they fit the task. Not every agentic workflow needs all of them.

- Each agent is a registered function with a single responsibility
- Agents communicate via **named queues** (ordered handoffs) and **shared state** (accumulated context)
- **Approval gates** are explicit checks in the producing agent before enqueuing the next step
- An HTTP trigger provides the entry point; agents chain from there
- **Pubsub** broadcasts completion events for downstream listeners

## Architecture

```text
HTTP request
  → Enqueue(agent-tasks) → Agent 1 (researcher) → writes state
    → Enqueue(agent-tasks) → Agent 2 (critic) → reads/updates state
      → explicit approval check (is-approved?)
        → Enqueue(agent-tasks) → Agent 3 (synthesizer) → final state update
          → publish(research.complete)
```

## iii Primitives Used

| Primitive                                                                    | Purpose                                      |
| ---------------------------------------------------------------------------- | -------------------------------------------- |
| `registerWorker`                                                             | Initialize the worker and connect to iii     |
| `registerFunction`                                                           | Define each agent                            |
| trigger `state::set`, `state::get`, `state::update`                 | Shared context between agents                |
| `trigger({ ..., action: TriggerAction.Enqueue({ queue }) })`                 | Async handoff between agents via named queue |
| `trigger({ function_id, payload })`                                          | Explicit condition check before enqueuing    |
| `trigger({ function_id: 'publish', payload, action: TriggerAction.Void() })` | Broadcast completion to any listeners        |
| `registerTrigger({ type: 'http' })`                                          | Entry point                                  |

## Reference Implementation

See [../references/agentic-backend.js](../references/agentic-backend.js) for the full working example — a multi-agent research pipeline
where a researcher gathers findings, a critic reviews them, and a synthesizer produces a final report.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `trigger({ function_id, payload, action: TriggerAction.Enqueue({ queue }) })` — async handoff between agents
- trigger `state::set`, `state::get`, `state::update` — shared context between agents
- Explicit condition check via `await iii.trigger({ function_id: 'condition-fn', payload })` before enqueuing next agent
- `trigger({ function_id: 'publish', payload: { topic, data }, action: TriggerAction.Void() })` — completion broadcast
- Each agent as its own `registerFunction` with `agents::` prefix IDs
- `const logger = new Logger()` — structured logging per agent

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Replace simulated logic in each agent with real work (API calls, LLM inference, etc.)
- Add more agents by registering functions and enqueuing to them with `TriggerAction.Enqueue({ queue })`
- For approval gates, call a condition function explicitly before enqueuing the next agent
- Define queue configs (retries, concurrency) in `iii-config.yaml` under `queue_configs`
- State scope should be named for your domain (e.g. `research-tasks`, `support-tickets`)
- `functionId` segments should reflect your agent hierarchy (e.g. `agents::researcher`, `agents::critic`)

## Engine Configuration

Named queues for agent handoffs are declared in iii-config.yaml under `queue_configs`. See [../references/iii-config.yaml](../references/iii-config.yaml) for the full annotated config reference.

## Pattern Boundaries

- If a request is about adapting existing HTTP endpoints into `registerFunction` (including prompts asking for `{ path, id }` endpoint maps + loops), prefer `iii-http-invoked-functions`.
- Stay with `iii-agentic-backend` when the primary problem is multi-agent orchestration, queue handoffs, approval gates, and shared context.

## When to Use

- Use this skill when the task is primarily about `iii-agentic-backend` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
