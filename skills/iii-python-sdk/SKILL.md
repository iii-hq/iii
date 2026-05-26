---
name: iii-python-sdk
description: >-
  Python SDK for the iii engine. Use when building workers, registering
  functions, or invoking triggers in Python.
---

# Python SDK

The Python SDK for connecting sync and async workers to the iii engine.

## Documentation

Full API reference: <https://iii.dev/docs/api-reference/sdk-python>

## Install

`pip install iii-sdk`

## Key Exports

| Export                                                     | Purpose                                          |
| ---------------------------------------------------------- | ------------------------------------------------ |
| `register_worker(address, options?)`                       | Connect to the engine and return the client      |
| `InitOptions(...)`                                         | Worker name, timeout, headers, reconnect, telemetry |
| `iii.register_function(id, handler, **options)`            | Register a sync or async local handler           |
| `iii.register_function(id, HttpInvocationConfig(...))`     | Register an external HTTP endpoint as a function |
| `iii.register_trigger({ type, function_id, config, metadata? })` | Bind a trigger to a function              |
| `iii.trigger(request)`                                     | Invoke a function synchronously                  |
| `await iii.trigger_async(request)`                         | Invoke a function asynchronously                 |
| `iii.create_channel()` / `await iii.create_channel_async()` | Create binary/text channels                    |
| `ChannelReader` / `ChannelWriter`                          | Consume/write channel payloads                   |
| `get_context()`                                            | Access logger and trace context inside handlers  |
| `Logger`                                                   | Structured logs                                  |
| `OtelConfig`                                               | OpenTelemetry options                            |
| `IIIInvocationError` / `IIIForbiddenError` / `IIITimeoutError` | SDK error classes                          |
| `on_connection_state_change(callback)`                     | Monitor connection state                         |

## Key Notes

- `register_worker()` returns a synchronous client.
- Handlers may be sync or async; sync handlers run in a worker thread.
- `ApiResponse` uses camelCase `statusCode` (pydantic alias), not `status_code`
- End workers with `while True: await asyncio.sleep(60)` to keep the event loop alive
- Use `asyncio.to_thread()` for CPU-heavy sync work inside handlers
- The SDK implements both `trigger_async(request)` and a synchronous `trigger(request)`. Use `trigger_async` inside async handlers, and `trigger` in synchronous scripts or threads where blocking behavior is desired.
- For discovery reads and topology subscriptions, call the built-in engine functions with `trigger()` / `trigger_async()` and bind `engine::functions-available` with `register_trigger()`. See `/docs/how-to/discover-workers-functions-triggers`.

## Functions, HTTP Invocation, and Metadata

```python
from iii import HttpInvocationConfig

iii.register_function(
    "orders::validate",
    validate_order,
    description="Validate an order",
    metadata={"owner": "orders"},
)

iii.register_function(
    "legacy::charge",
    HttpInvocationConfig(
        url="https://legacy.example.com/charge",
        method="POST",
        timeout_ms=5000,
        auth={"type": "api_key", "header": "X-API-Key", "value_key": "LEGACY_API_KEY"},
    ),
)
```

HTTP auth supports `hmac`, `bearer`, and `api_key`. Pass environment variable names in `secret_key`, `token_key`, or `value_key`, not raw secrets.

## Channels

- `channel = iii.create_channel()` in sync code.
- `channel = await iii.create_channel_async()` in async code.
- Pass `channel.reader_ref` or `channel.writer_ref` through trigger payloads.
- `ChannelWriter.write(bytes)` sends binary chunks; `send_message()` sends text.
- `ChannelReader` supports `read_all()`, `on_message(callback)`, and async iteration for incremental binary chunks.

## Errors

Catch `IIIInvocationError` for remote failures. `IIIForbiddenError` maps to `FORBIDDEN`; `IIITimeoutError` maps to `TIMEOUT`. All invocation errors expose `code`, `message`, `function_id`, `stacktrace`, and `invocation_id` when the engine provides them.

## Examples

```python
# Async invocation (non-blocking, typical inside handlers)
result = await iii.trigger_async({
    "function_id": "greet",
    "payload": {"name": "World"}
})

# Sync invocation (blocks the current thread, useful in sync contexts)
result = iii.trigger({
    "function_id": "greet",
    "payload": {"name": "World"}
})
```

## Pattern Boundaries

- For usage patterns and working examples, see `iii-functions-and-triggers`
- For channels, see `iii-channels`
- For errors, see `iii-error-handling`
- For Node.js SDK, see `iii-node-sdk`
- For Rust SDK, see `iii-rust-sdk`
- For browser-side usage, see `iii-browser-sdk`

## When to Use

- Use this skill when the task is primarily about `iii-python-sdk` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
