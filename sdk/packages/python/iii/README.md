# iii-sdk

Python SDK for the [iii engine](https://github.com/iii-hq/iii).

[![PyPI](https://img.shields.io/pypi/v/iii-sdk)](https://pypi.org/project/iii-sdk/)
[![Python](https://img.shields.io/pypi/pyversions/iii-sdk)](https://pypi.org/project/iii-sdk/)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](../../../LICENSE)

## Prerequisites

The SDK connects to a running iii engine over WebSocket. Before running the snippets below:

1. Install the engine: `curl -fsSL https://install.iii.dev/iii/main/install.sh | sh`
2. Start the engine in a separate terminal: `iii --config config.yaml`
   (default WebSocket URL: `ws://localhost:49134`)

See the [iii quickstart](https://iii.dev/docs/quickstart) for scaffolding a full project.

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

iii.register_function("greet", greet)

iii.register_trigger({
    "type": "http",
    "function_id": "greet",
    "config": {"api_path": "/greet", "http_method": "POST"},
})

result = iii.trigger({"function_id": "greet", "payload": {"name": "world"}})
print(result)  # {"message": "Hello, world!"}
```

`register_worker()` auto-connects to the engine and blocks until the connection is
established. There is no separate `connect()` step.

## API

| Operation                | Signature                                         | Description                                            |
| ------------------------ | ------------------------------------------------- | ------------------------------------------------------ |
| Initialize               | `register_worker(url, options?)`                  | Create an SDK instance and auto-connect                |
| Register function        | `iii.register_function(id, handler)`              | Register a function that can be invoked by name        |
| Register trigger         | `iii.register_trigger({"type": ..., "function_id": ..., "config": ...})` | Bind a trigger (HTTP, cron, queue, etc.) to a function |
| Invoke (await result)    | `iii.trigger({"function_id": id, "payload": data})` | Invoke a function and wait for the result           |
| Invoke (fire-and-forget) | `iii.trigger({"function_id": id, ..., "action": TriggerAction.Void()})` | Fire-and-forget |
| Shutdown                 | `iii.shutdown()`                                  | Disconnect and stop background thread                  |

### Registering Functions

```python
def create_order(data):
    return {"status_code": 201, "body": {"id": "123", "item": data["body"]["item"]}}

iii.register_function("orders.create", create_order)
```

### Registering Triggers

```python
iii.register_trigger({
    "type": "http",
    "function_id": "orders.create",
    "config": {"api_path": "/orders", "http_method": "POST"},
})
```

### Invoking Functions

```python
result = iii.trigger({"function_id": "orders.create", "payload": {"body": {"item": "widget"}}})
```

### Advanced: dict-form registration

`register_function` also accepts a `RegisterFunctionInput` or a dict with `id`
for callers that need to pass extra registration fields inline:

```python
iii.register_function({"id": "orders.create", "description": "Create a new order"}, create_order)
```

Prefer the string form above for hello-world-style examples.

## Modules

| Import          | What it provides                  |
| --------------- | --------------------------------- |
| `iii`           | Core SDK (`III`, types)           |
| `iii.stream`    | Stream client for real-time state |
| `iii.telemetry` | OpenTelemetry integration         |

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
