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

result = iii.trigger({"function_id": "hello::greet", "payload": {"name": "world"}})
print(result)  # {"message": "Hello, world!"}
```

## API

| Operation                | Signature                                                                              | Description                                            |
| ------------------------ | -------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| Initialize               | `register_worker(url, options?)`                                                       | Create an SDK instance and auto-connect                |
| Register function        | `iii.register_function(id, handler)`                                                   | Register a function that can be invoked by name        |
| Invoke (await result)    | `iii.trigger({"function_id": id, "payload": data})`                                    | Invoke a function and wait for the result              |
| Invoke (fire-and-forget) | `iii.trigger({"function_id": id, ..., "action": TriggerAction.Void()})`                | Fire-and-forget                                        |
| Invoke (enqueue)         | `iii.trigger({"function_id": id, ..., "action": TriggerAction.Enqueue(queue="name")})` | Route invocation through a named queue                 |
| Observe                  | `iii.observe({"function_id": ..., "args": ..., "filters": ...}, target, options?)`     | Invoke `target` on every matching event of a function  |
| Current invocation       | `iii.current_invocation()`                                                             | The invocation the running handler serves              |
| Shutdown                 | `iii.shutdown()`                                                                       | Disconnect and stop background thread                  |

`register_worker()` creates the SDK instance and auto-connects to the engine.

### Registering Functions

```python
def create_order(data):
    return {"status_code": 201, "body": {"id": "123", "item": data["body"]["item"]}}

iii.register_function("orders::create", create_order)
```

### Invoking Functions

```python
result = iii.trigger({"function_id": "orders::create", "payload": {"body": {"item": "widget"}}})
```

`trigger()` blocks the calling thread; `await iii.trigger_async(...)` works from
any event loop. A call to a function this worker registered itself runs
in-process once the engine confirmed it (no frames on the wire unless someone
observes it): a sync handler on the calling thread, an async one on the
calling loop. Pass `"dispatch": "remote"` to go through the engine anyway, or
register the function with `local_dispatch=False`.

### Observing Functions

Every function is observable. The target is a registered function; it receives
the `Observed` envelope (`input`, `output` or `error`, `caller`, ...).

```python
def big_order(envelope):
    notify(envelope["input"], envelope["output"])

iii.register_function("alerts::big-order", big_order)

observer = iii.observe(
    {"function_id": "orders::create", "filters": {"input.body.total": {"$gte": 500}}},
    "alerts::big-order",
    {"max_in_flight": 4},
)
observer.ready()            # or: await observer.ready_async()
# later
observer.dispose()
```

A function can hand its own events to the observers they belong to (owner
delivery), and react when one is added or removed:

```python
from iii import ObserveHooks, Observer

schedules: dict[str, set[Observer]] = {}  # expression -> observers

class CronHooks(ObserveHooks):
    delivery = "owner"
    args_format = {"type": "object", "required": ["expression"],
                   "properties": {"expression": {"type": "string"}}}

    def on_observer_added(self, obs):
        schedules.setdefault(obs.args["expression"], set()).add(obs)

    def on_observer_removed(self, obs):
        schedules[obs.args["expression"]].discard(obs)

def on_tick(tick):
    for obs in schedules.get(tick["expression"], ()):
        obs.emit(tick)  # the observer's filters are checked here

iii.register_function("cron::on-tick", on_tick, observe=CronHooks())
```

Inside any handler, `iii.current_invocation()` returns the running invocation
(`function_id`, `invocation_id`, `caller`, `activation`, `observe_depth`). Sync
handlers run on a thread of their own started in a copy of the caller's
context, so the current invocation and the OpenTelemetry context follow them.

### Trigger APIs (deprecated)

`register_trigger` and `register_trigger_type` keep working in 0.25 but are
deprecated: they emit a `DeprecationWarning` and log a warning once per
process. Use `iii.observe(source, target)` and
`register_function(id, handler, observe=ObserveHooks(...))` instead. See the
[migration guide](https://iii.dev/docs/upgrading/from-0-24-x).

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
