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

| Export                                                   | Purpose                                           |
| -------------------------------------------------------- | ------------------------------------------------- |
| `registerWorker(url, { workerName })`                    | Connect to the engine and return the client       |
| `registerFunction(id, handler, options?)`                | Register a local async function handler           |
| `registerFunction(id, httpConfig, options?)`             | Register an HTTP-invoked external function        |
| `registerTrigger({ type, function_id, config, metadata? })` | Bind a trigger to a function (with optional metadata) |
| `trigger({ function_id, payload, action? })`             | Invoke a function                                 |
| `TriggerAction.Void()`                                   | Fire-and-forget invocation mode                   |
| `TriggerAction.Enqueue({ queue })`                       | Durable async invocation mode                     |
| `Logger`                                                 | Structured logging                                |
| `withSpan`, `getTracer`, `getMeter`                      | OpenTelemetry instrumentation                     |
| `createChannel()`                                        | Binary streaming between workers                  |
| `createStream(name, adapter)`                            | Custom stream implementation                      |
| `registerTriggerType(id, handler)`                       | Custom trigger type registration                  |

## RBAC Auth Result Fields

When implementing an auth function for RBAC workers, the `AuthResult` supports:

| Field                              | Purpose                                            |
| ---------------------------------- | -------------------------------------------------- |
| `allowed_functions: string[]`      | Additional function IDs to allow                   |
| `forbidden_functions: string[]`    | Function IDs to deny (overrides expose_functions)  |
| `allowed_trigger_types?: string[]` | Trigger types the worker may register              |
| `allow_trigger_type_registration`  | Whether the worker can register new trigger types  |
| `function_registration_prefix?`    | Prefix applied to functions registered by worker   |
| `context: Record<string, unknown>` | Arbitrary context forwarded to middleware/handlers  |

## Browser SDK

For browser environments, use `iii-sdk-browser` (same API, adapted for browser WebSocket constraints). See `iii-browser-sdk` skill for details.

## Pattern Boundaries

- For usage patterns and working examples, see `iii-functions-and-triggers`
- For HTTP endpoint patterns, see `iii-http-endpoints`
- For HTTP middleware patterns, see `iii-http-middleware`
- For browser-side usage, see `iii-browser-sdk`
- For Python SDK, see `iii-python-sdk`
- For Rust SDK, see `iii-rust-sdk`

## When to Use

- Use this skill when the task is primarily about `iii-node-sdk` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
