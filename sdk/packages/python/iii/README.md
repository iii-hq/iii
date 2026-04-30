# iii-sdk

Python SDK for the [iii engine](https://github.com/iii-hq/iii).

[![PyPI](https://img.shields.io/pypi/v/iii-sdk)](https://pypi.org/project/iii-sdk/)
[![Python](https://img.shields.io/pypi/pyversions/iii-sdk)](https://pypi.org/project/iii-sdk/)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](../../../LICENSE)

## Install

```bash
pip install iii-sdk
```

## Hello World

```python
from iii import register_worker

iii = register_worker("ws://localhost:49134")

def greet(data):
    return {"message": f"Hello, {data['name']}!"}

iii.register_function("hello::greet", greet)

iii.register_trigger({
    "type": "http",
    "function_id": "hello::greet",
    "config": {"api_path": "/greet", "http_method": "POST"},
})

iii.connect()

result = iii.trigger({"function_id": "hello::greet", "payload": {"name": "world"}})
print(result)  # {"message": "Hello, world!"}
```

## API

| Operation                | Signature                                                                              | Description                                            |
| ------------------------ | -------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| Initialize               | `register_worker(url, options?)`                                                       | Create an SDK instance and auto-connect                |
| Register function        | `iii.register_function(id, handler)`                                                   | Register a function that can be invoked by name        |
| Register trigger         | `iii.register_trigger({"type": ..., "function_id": ..., "config": ...})`               | Bind a trigger (HTTP, cron, queue, etc.) to a function |
| Invoke (await result)    | `iii.trigger({"function_id": id, "payload": data})`                                    | Invoke a function and wait for the result              |
| Invoke (fire-and-forget) | `iii.trigger({"function_id": id, ..., "action": TriggerAction.Void()})`                | Fire-and-forget                                        |
| Invoke (enqueue)         | `iii.trigger({"function_id": id, ..., "action": TriggerAction.Enqueue(queue="name")})` | Route invocation through a named queue                 |
| Shutdown                 | `iii.shutdown()`                                                                       | Disconnect and stop background thread                  |

`register_worker()` creates the SDK instance and auto-connects to the engine.

### Registering Functions

```python
def create_order(data):
    return {"status_code": 201, "body": {"id": "123", "item": data["body"]["item"]}}

iii.register_function("orders::create", create_order)
```

### Registering Triggers

```python
iii.register_trigger({
    "type": "http",
    "function_id": "orders::create",
    "config": {"api_path": "/orders", "http_method": "POST"},
})
```

### Invoking Functions

```python
result = iii.trigger({"function_id": "orders::create", "payload": {"body": {"item": "widget"}}})
```

## Development

### Install in development mode

```bash
pip install -e .
```

### Type checking

```bash
mypy src
```

### Linting

```bash
ruff check src
```

## Resources

- [Documentation](https://iii.dev/docs)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)
