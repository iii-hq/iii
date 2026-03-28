---
name: node-sdk
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

- For usage patterns and working examples, see `functions-and-triggers`
- For HTTP endpoint patterns, see `http-endpoints`
- For Python SDK, see `python-sdk`
- For Rust SDK, see `rust-sdk`
