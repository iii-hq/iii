---
name: rust-sdk
description: >-
  Rust SDK for the iii engine. Use when building high-performance workers,
  registering functions, or invoking triggers in Rust.
---

# Rust SDK

The native async Rust SDK for connecting workers to the iii engine via tokio.

## Documentation

Full API reference: <https://iii.dev/docs/api-reference/sdk-rust>

## Install

Add to `Cargo.toml`:

`iii-sdk = { version = "0.10", features = ["otel"] }`

## Key Types and Functions

| Export                                             | Purpose                                                                          |
| -------------------------------------------------- | -------------------------------------------------------------------------------- |
| `register_worker(url, InitOptions)`                | Connect to the engine, returns `III` client                                      |
| `III::register_function(RegisterFunction::new(id, handler))` | Register a sync function using the builder API                          |
| `III::register_function(RegisterFunction::new_async(id, handler))` | Register an async function using the builder API                    |
| `III::register_function_with(msg, handler)`        | Two-arg convenience method for function registration                             |
| `RegisterFunction`                                 | Builder with `.description()` and auto-generated request schemas via `schemars`  |
| `III::register_trigger(type, function_id, config)` | Bind a trigger to a function                                                     |
| `III::trigger(TriggerRequest)`                     | Invoke a function                                                                |
| `TriggerAction::Void`                              | Fire-and-forget invocation                                                       |
| `TriggerAction::Enqueue { queue }`                 | Durable async invocation                                                         |
| `IIIError`                                         | Error type for handler failures                                                  |
| `Streams`                                          | Helper for atomic stream CRUD                                                    |
| `with_span`, `get_tracer`, `get_meter`             | OpenTelemetry (requires `otel` feature)                                          |
| `execute_traced_request`                           | HTTP client with trace context propagation                                       |

## Key Notes

- Add `features = ["otel"]` to `Cargo.toml` for OpenTelemetry support
- Use `RegisterFunction::new("id", handler)` for sync handlers, `RegisterFunction::new_async("id", handler)` for async
- Handler input types that derive `schemars::JsonSchema` get auto-generated request schemas
- Chain `.description("...")` on `RegisterFunction` to document the function
- Keep the tokio runtime alive (e.g., `tokio::time::sleep` loop) for event processing
- `register_trigger` returns `Ok(())` on success; propagate errors with `?`

## Pattern Boundaries

- For usage patterns and working examples, see `functions-and-triggers`
- For Node.js SDK, see `node-sdk`
- For Python SDK, see `python-sdk`

## When to Use

- Use this skill when the task is primarily about `rust-sdk` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
