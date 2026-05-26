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

Add to `Cargo.toml`:

```toml
iii-sdk = "0.13"
# Optional: OpenTelemetry + Logger primitives live in a separate crate.
iii-observability = "0.13"
```

## Key Types and Functions

| Export                                             | Purpose                                                                          |
| -------------------------------------------------- | -------------------------------------------------------------------------------- |
| `register_worker(url, InitOptions)`                | Connect to the engine, returns `III` client                                      |
| `III::register_function(id, RegisterFunction::new(handler))` | Register a sync function. Accepts typed handlers (schemas auto-extracted via `schemars`) and `Fn(Value) -> Result<Value, IIIError>` closures. Handler error type must be `IIIError`. |
| `III::register_function(id, RegisterFunction::new_async(handler))` | Async equivalent of `new`. Same dual-shape support. |
| `III::register_function(id, RegisterFunction::http(http_config))` | Register an HTTP-invoked function (Lambda, Workers, etc.) — no local handler. |
| `RegisterFunction`                                 | Builder with `.description()`, `.metadata()`, `.request_format()`, `.response_format()` |
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

- For usage patterns and working examples, see `iii-functions-and-triggers`
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
