---
name: iii-rust-sdk
description: >-
  Rust SDK for the iii engine. Use when building high-performance workers,
  registering functions, or invoking triggers in Rust.
---

# Rust SDK

The native async Rust SDK for connecting workers to the iii engine via tokio.

## Documentation

Full API reference: <https://iii.dev/docs/api-reference/sdk-rust>

## Install

Use Cargo:

```bash
cargo add iii-sdk
```

## Key Types and Functions

| Export                                                        | Purpose                                                                         |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `register_worker(url, InitOptions)`                           | Connect to the engine, returns `III` client                                     |
| `InitOptions { metadata, headers, otel }`                     | Worker metadata, auth headers, OpenTelemetry config                             |
| `III::register_function(RegisterFunction::new(id, handler))`  | Register a sync function using the builder API                                  |
| `III::register_function(RegisterFunction::new_async(id, handler))` | Register an async function using the builder API                          |
| `III::register_function_with(message, handler_or_http_config)` | Register from a full message, including HTTP-invoked functions                 |
| `RegisterFunction`                                            | Builder with `.description()` and `.metadata()`                                 |
| `HttpInvocationConfig` / `HttpAuthConfig`                     | External HTTP endpoint invocation config                                        |
| `IIITrigger`                                                  | Typed built-in trigger builders                                                 |
| `RegisterTriggerType` / `TriggerTypeRef`                      | Typed custom trigger type builders                                              |
| `III::register_trigger(RegisterTriggerInput)`                 | Bind a trigger to a function                                                    |
| `III::trigger(TriggerRequest)`                                | Invoke a function                                                               |
| `TriggerAction::Void`                                         | Fire-and-forget invocation                                                      |
| `TriggerAction::Enqueue { queue }`                            | Durable async invocation                                                        |
| `IIIError`                                                    | SDK, handler, timeout, and remote error type                                    |
| `ChannelReader` / `ChannelWriter` / `extract_channel_refs()`  | Binary/text channel APIs                                                        |
| `Logger`                                                     | Structured logs                                                                 |
| `with_span`, `run_in_span`, `OtelConfig`                      | OpenTelemetry instrumentation                                                   |
| `execute_traced_request`                                      | HTTP client with trace context propagation                                      |

## Key Notes

- Use `RegisterFunction::new("id", handler)` for sync handlers, `RegisterFunction::new_async("id", handler)` for async
- Handler input/output types that derive `schemars::JsonSchema` get auto-generated request and response schemas
- Chain `.description("...")` and `.metadata(json!(...))` on `RegisterFunction` to document the function
- Keep the tokio runtime alive (e.g., `tokio::time::sleep` loop) for event processing
- `register_trigger` returns `Ok(())` on success; propagate errors with `?`

## HTTP-Invoked Functions

Use `register_function_with(RegisterFunctionMessage, HttpInvocationConfig)` when a function ID should call an external HTTP endpoint instead of a local Rust handler.

Auth modes:

- `HttpAuthConfig::Hmac { secret_key }`
- `HttpAuthConfig::Bearer { token_key }`
- `HttpAuthConfig::ApiKey { header, value_key }`

`secret_key`, `token_key`, and `value_key` name environment variables. Do not put raw credentials in config.

## Typed Trigger Builders

- `RegisterTriggerType::new(id, description, handler)` registers a custom trigger type.
- Chain `.trigger_request_format::<T>()` for trigger config schema and `.call_request_format::<T>()` for function payload schema.
- The returned `TriggerTypeRef<C, R>` has typed `register_trigger`, `register_trigger_with_metadata`, `register_function`, and `register_function_async`.
- Use `IIITrigger` for typed built-in trigger builders when authoring Rust workers that bind HTTP, cron, queue, state, stream, or log triggers.

## Channels

- `let channel = iii.create_channel(None).await?`
- Pass `channel.reader_ref` or `channel.writer_ref` through a trigger payload.
- Use `ChannelReader::new(engine_ws_base, &reader_ref)` and `ChannelWriter::new(engine_ws_base, &writer_ref)` when reconstructing refs from payloads.
- Use `next_binary()` for incremental chunks, `read_all()` for full buffers, and `send_message()` for text messages.
- Use `extract_channel_refs(&serde_json::Value)` to find refs inside nested payloads.

## Errors

`IIIError::Remote { code, message, stacktrace }` carries engine error codes such as `FORBIDDEN`, `TIMEOUT`, `function_not_found`, `function_not_invokable`, `invocation_failed`, and `invocation_stopped`. `IIIError::Timeout`, `IIIError::NotConnected`, `IIIError::Handler`, `IIIError::Serde`, and `IIIError::WebSocket` describe SDK/local failures.

## Pattern Boundaries

- For usage patterns and working examples, see `iii-functions-and-triggers`
- For HTTP middleware patterns, see `iii-http-middleware`
- For channels, see `iii-channels`
- For errors, see `iii-error-handling`
- For Node.js SDK, see `iii-node-sdk`
- For Python SDK, see `iii-python-sdk`
- For browser-side usage, see `iii-browser-sdk`

## When to Use

- Use this skill when the task is primarily about `iii-rust-sdk` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
