---
name: iii-functions-and-triggers
description: >-
  Registers functions and triggers on the iii engine across TypeScript, Python,
  and Rust. Use when creating workers, registering function handlers, binding
  triggers, or invoking functions across languages.
---

# Functions & Triggers

Comparable to: Serverless function runtimes, Lambda, Cloud Functions

## Key Concepts

Use the concepts below when they fit the task. Not every worker needs all of them.

- A **Function** is an async handler registered with a unique ID
- A **Trigger** binds an event source to a function — types include http, queue, cron, state, stream, and subscribe
- Functions invoke other functions via `trigger()` regardless of language or worker location
- The engine handles serialization, routing, and delivery automatically
- HTTP-invoked functions wrap external endpoints as callable function IDs
- Functions can declare **request/response formats** for documentation and discovery — auto-generated from types in Rust (via `schemars::JsonSchema`) and Python (via type hints / Pydantic), or manually provided in Node.js

## Architecture

`registerWorker()` connects the worker to the engine, `registerFunction` defines handlers, `registerTrigger` binds event sources to those handlers, and the engine routes incoming events to the correct function. Functions can invoke other functions across workers and languages via `trigger()`.

## iii Primitives Used

| Primitive                                                    | Purpose                            |
| ------------------------------------------------------------ | ---------------------------------- |
| `registerWorker(url, options?)`                              | Connect worker to engine           |
| `registerFunction({ id }, handler)`                          | Define a function handler          |
| `registerTrigger({ type, function_id, config, metadata? })`  | Bind an event source to a function |
| `trigger({ function_id, payload })`                          | Invoke a function synchronously    |
| `trigger({ ..., action: TriggerAction.Void() })`             | Fire-and-forget invocation         |
| `trigger({ ..., action: TriggerAction.Enqueue({ queue }) })` | Durable async invocation via queue |

## Reference Implementation

- **TypeScript**: [../references/functions-and-triggers.js](../references/functions-and-triggers.js)
- **Python**: [../references/functions-and-triggers.py](../references/functions-and-triggers.py)
- **Rust**: [../references/functions-and-triggers.rs](../references/functions-and-triggers.rs)

Each reference shows the same patterns (function registration, trigger binding, cross-function invocation) in its respective language.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker('ws://localhost:49134', { workerName: 'my-worker' })` — connect to the engine
- `registerFunction({ id: 'namespace::name' }, async (input) => { ... })` — register a handler
- `registerTrigger({ type: 'http', function_id, config: { api_path, http_method, middleware_function_ids? } })` — HTTP trigger (with optional middleware)
- `registerTrigger({ type: 'queue', function_id, config: { topic } })` — queue trigger
- `registerTrigger({ type: 'cron', function_id, config: { expression } })` — cron trigger
- `registerTrigger({ type: 'state', function_id, config: { scope, key } })` — state change trigger
- `registerTrigger({ type: 'stream', function_id, config: { stream } })` — stream trigger
- `registerTrigger({ type: 'subscribe', function_id, config: { topic } })` — pubsub subscriber
- Cross-language invocation: a TypeScript function can trigger a Python or Rust function by ID
- `registerTrigger({ ..., metadata: { owner: 'team', priority: 'high' } })` — optional trigger metadata

### Request/Response Format (Auto-Registration)

Functions can declare their input/output schemas for documentation and discovery:

- **Rust**: Derive `schemars::JsonSchema` on handler input/output types — `RegisterFunction::new()` auto-generates JSON Schema (Draft 7) from the type
- **Python**: Use type hints (Pydantic models or primitives) on handler parameters and return types — `register_function()` auto-extracts JSON Schema (Draft 2020-12)
- **Node.js**: Pass `request_format` / `response_format` manually in the registration message (e.g., via Zod's `toJSONSchema()`)

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Replace placeholder handler logic with real business logic (API calls, DB queries, LLM calls)
- Use `namespace::name` convention for function IDs to group related functions
- For HTTP endpoints, configure `api_path` and `http_method` in the trigger config
- For durable async work, use `TriggerAction.Enqueue({ queue })` instead of synchronous trigger
- For fire-and-forget side effects, use `TriggerAction.Void()`
- Multiple workers in different languages can register functions that invoke each other by ID

## Pattern Boundaries

- For HTTP endpoint specifics (request/response format, path params), prefer `iii-http-endpoints`.
- For queue processing details (retries, concurrency, FIFO), prefer `iii-queue-processing`.
- For cron scheduling details (expressions, timezones), prefer `iii-cron-scheduling`.
- For invocation modes (sync vs void vs enqueue), prefer `iii-trigger-actions`.
- Stay with `iii-functions-and-triggers` when the primary problem is registering functions, binding triggers, or cross-language invocation.

## When to Use

- Use this skill when the task is primarily about `iii-functions-and-triggers` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
