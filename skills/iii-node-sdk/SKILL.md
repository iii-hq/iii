---
name: iii-node-sdk
description: >-
  Node.js/TypeScript SDK for the iii engine. Use when building workers,
  registering functions, or invoking triggers in TypeScript or JavaScript.
---

# Node.js SDK

The TypeScript/JavaScript SDK for connecting workers to the iii engine.

## Documentation

Full API reference: <https://iii.dev/docs/api-reference/sdk-node>

## Install

`npm install iii-sdk`

## Key Exports

| Export                                           | Purpose                                     |
| ------------------------------------------------ | ------------------------------------------- |
| `registerWorker(url, { workerName })`            | Connect to the engine and return the client |
| `registerFunction({ id }, handler)`              | Register an async function handler          |
| `registerTrigger({ type, function_id, config })` | Bind a trigger to a function                |
| `trigger({ function_id, payload, action? })`     | Invoke a function                           |
| `TriggerAction.Void()`                           | Fire-and-forget invocation mode             |
| `TriggerAction.Enqueue({ queue })`               | Durable async invocation mode               |
| `Logger`                                         | Structured logging                          |
| `withSpan`, `getTracer`, `getMeter`              | OpenTelemetry instrumentation               |
| `createChannel()`                                | Binary streaming between workers            |
| `createStream(name, adapter)`                    | Custom stream implementation                |
| `registerTriggerType(id, handler)`               | Custom trigger type registration            |

## Pattern Boundaries

- For usage patterns and working examples, see `iii-functions-and-triggers`
- For HTTP endpoint patterns, see `iii-http-endpoints`
- For Python SDK, see `iii-python-sdk`
- For Rust SDK, see `iii-rust-sdk`

## When to Use

- Use this skill when the task is primarily about `iii-node-sdk` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
