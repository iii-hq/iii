---
name: iii-node-sdk
description: >-
  Node.js/TypeScript SDK for the iii engine. Use when building workers,
  registering functions, or invoking triggers in TypeScript or JavaScript.
---

# Node.js SDK

The TypeScript/JavaScript SDK for connecting Node workers to the iii engine.

## Documentation

Full API reference: <https://iii.dev/docs/api-reference/sdk-node>

## Install

`npm install iii-sdk`

## Key APIs

| API                                                                 | Purpose                                            |
| ------------------------------------------------------------------- | -------------------------------------------------- |
| `registerWorker(url, options?)`                                     | Connect to the engine and return the `iii` client  |
| `InitOptions`                                                       | Worker name, metrics, headers, reconnect, OpenTelemetry |
| `iii.registerFunction(id, handler, options?)`                       | Register a local async function handler            |
| `iii.registerFunction(id, HttpInvocationConfig, options?)`          | Register an external HTTP endpoint as a function   |
| `iii.registerTrigger({ type, function_id, config, metadata? })`     | Bind a trigger to a function                       |
| `iii.trigger({ function_id, payload, action?, timeoutMs? })`        | Invoke a function                                  |
| `TriggerAction.Void()`                                              | Fire-and-forget invocation mode                    |
| `TriggerAction.Enqueue({ queue })`                                  | Durable async invocation mode                      |
| `iii.createChannel(bufferSize?)`                                    | Binary/text streaming between workers              |
| `ChannelReader` / `ChannelWriter`                                   | Consume/write channel payloads                     |
| `Logger`                                                           | Structured logs with OpenTelemetry fallback        |
| `withSpan`                                                          | OpenTelemetry custom spans                         |
| `IIIInvocationError`                                                | Remote invocation error with `code` and `function_id` |
| `iii.registerTriggerType({ id, description }, handler)`             | Custom trigger type registration                   |

## InitOptions

`registerWorker(address, options?)` supports:

- `workerName`
- `enableMetricsReporting`
- `invocationTimeoutMs`
- `reconnectionConfig` (`initialDelayMs`, `maxDelayMs`, `backoffMultiplier`, `jitterFactor`, `maxRetries`)
- `headers`
- `otel`

Use headers only for server-side Node workers. Browser workers cannot send custom WebSocket headers.

## Functions and Metadata

- Local handler: `iii.registerFunction('orders::validate', async (payload) => result, { description, metadata, request_format, response_format })`
- HTTP-invoked handler: `iii.registerFunction('legacy::charge', { url, method, timeout_ms, headers, auth }, options?)`
- `metadata` is discoverable through engine function listings and should contain stable ownership or capability data, not secrets.
- HTTP auth supports `{ type: 'hmac', secret_key }`, `{ type: 'bearer', token_key }`, and `{ type: 'api_key', header, value_key }`.

## Trigger Actions and Errors

- Default `trigger()` waits for the result and rejects on handler/engine errors.
- `TriggerAction.Void()` returns `undefined` after dispatch; use only for optional side effects.
- `TriggerAction.Enqueue({ queue })` returns `{ messageReceiptId }` after the job is accepted.
- `timeoutMs` overrides the invocation timeout for a single call.
- Catch `IIIInvocationError`; inspect `error.code` for `FORBIDDEN`, `TIMEOUT`, `function_not_found`, `function_not_invokable`, `invocation_failed`, and `invocation_stopped`.

## Channels

- `const channel = await iii.createChannel()`
- Pass `channel.readerRef` or `channel.writerRef` through a `trigger()` payload.
- Write binary chunks with `channel.writer.stream.write(buffer)` or text messages with `channel.writer.sendMessage(text)`.
- Read binary data with `reader.stream`, `reader.readAll()`, and text messages with `reader.onMessage(callback)`.

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

For browser environments, use `iii-browser-sdk`. It has browser-specific security and WebSocket constraints; see `iii-browser-sdk` for details.

## Pattern Boundaries

- For usage patterns and working examples, see `iii-functions-and-triggers`
- For browser-side usage, see `iii-browser-sdk`
- For channels, see `iii-channels`
- For errors, see `iii-error-handling`
- For Python SDK, see `iii-python-sdk`
- For Rust SDK, see `iii-rust-sdk`

## When to Use

- Use this skill when the task is primarily about `iii-node-sdk` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
