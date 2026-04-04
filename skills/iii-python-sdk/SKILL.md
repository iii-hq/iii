---
name: iii-python-sdk
description: >-
  Python SDK for the iii engine. Use when building workers, registering
  functions, or invoking triggers in Python.
---

# Python SDK

The async Python SDK for connecting workers to the iii engine.

## Documentation

Full API reference: <https://iii.dev/docs/api-reference/sdk-python>

## Install

`pip install iii-sdk`

## Key Exports

| Export                                        | Purpose                                         |
| --------------------------------------------- | ----------------------------------------------- |
| `register_worker(address, options?)`          | Connect to the engine, returns the client       |
| `InitOptions(worker_name, otel?)`             | Connection configuration                        |
| `register_function(id, handler)`              | Register an async function handler              |
| `register_trigger(type, function_id, config)` | Bind a trigger to a function                    |
| `trigger(request)`                            | Invoke a function synchronously                 |
| `trigger_async(request)`                      | Invoke a function asynchronously                |
| `get_context()`                               | Access logger and trace context inside handlers |
| `ApiRequest` / `ApiResponse`                  | HTTP request/response types (pydantic)          |
| `IStream`                                     | Interface for custom stream implementations     |
| `on_functions_available(callback)`            | Listen for function discovery                   |
| `on_connection_state_change(callback)`        | Monitor connection state                        |
| `register_trigger(type, fn_id, config, metadata?)` | Bind a trigger with optional metadata      |

## Key Notes

- `register_worker()` returns a synchronous client; handlers are async
- `ApiResponse` uses camelCase `statusCode` (pydantic alias), not `status_code`
- End workers with `while True: await asyncio.sleep(60)` to keep the event loop alive
- Use `asyncio.to_thread()` for CPU-heavy sync work inside handlers
- The SDK implements both `trigger_async(request)` and a synchronous `trigger(request)`. Use `trigger_async` inside async handlers, and `trigger` in synchronous scripts or threads where blocking behavior is desired.

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
- For HTTP middleware patterns, see `iii-http-middleware`
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
