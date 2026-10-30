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

result = iii.call({"function_id": "hello::greet", "payload": {"name": "world"}})
print(result)  # {"message": "Hello, world!"}
```

## API

| Operation                | Signature                                                                                  | Description                                              |
| ------------------------ | ------------------------------------------------------------------------------------------ | -------------------------------------------------------- |
| Initialize               | `register_worker(url, options?)`                                                           | Create an SDK instance and auto-connect                  |
| Register function        | `iii.register_function(id, handler, **options)`                                            | Register a function that can be called by name           |
| Call (await result)      | `iii.call({"function_id": id, "payload": data})`                                           | Call a function and wait for the result                  |
| Call (fire-and-forget)   | `iii.call({"function_id": id, ..., "action": CallAction.Void()})`                          | Fire-and-forget                                          |
| Call (enqueue)           | `iii.call({"function_id": id, ..., "action": CallAction.Enqueue(queue="name")})`           | Route the call through a named queue                     |
| Call through an observer | `iii.call({"observer": obs, "payload": data})`                                             | Run an observer's target with its policy                 |
| Observe                  | `iii.observe("fn::id", target, options?)`                                                  | Run `target` when the function runs                      |
| Observer hooks           | `iii.on_observer_added`, `on_observer_removed`, `on_observer_match`, `on_observer_deliver` | Decide what observing a function this worker hosts means |
| Current invocation       | `iii.current_invocation()`                                                                 | The invocation the running handler serves                |
| Shutdown                 | `iii.shutdown()`                                                                           | Disconnect and stop background thread                    |

`register_worker()` creates the SDK instance and auto-connects to the engine.
`trigger` and `trigger_async` are deprecated aliases of `call` and
`call_async`.

### Registering Functions

```python
def create_order(data):
    return {"status_code": 201, "body": {"id": "123", "item": data["body"]["item"]}}

iii.register_function("orders::create", create_order)
```

The handler's return value is what the function's observers observe. Options:
`description`, `metadata`, `request_format`, `response_format` (extracted from
type hints when omitted), `args_format` (a JSON Schema or Pydantic model of
the observers' `args`), `private=True` (only this worker may call it; its own
calls pass the input and result by reference), `observable=False` (nobody may
observe it), `standby=True` (take the function over when the worker that hosts
it leaves) and `local_dispatch`.

### Calling Functions

```python
result = iii.call({"function_id": "orders::create", "payload": {"body": {"item": "widget"}}})
```

`call()` blocks the calling thread; `await iii.call_async(...)` works from any
event loop. A call to a function this worker registered itself runs in-process
once the engine confirmed it (no frames on the wire unless someone observes
it): a sync handler on a thread of its own, an async one on the calling loop.
Pass `"dispatch": "remote"` to go through the engine anyway, or register the
function with `local_dispatch=False`.

### Observing Functions

When `orders::create` runs, `alerts::big-order` runs:

```python
iii.observe("orders::create", "alerts::big-order")
```

The target is a registered function; it receives the `Observed` envelope
(`input`, `output` or `error`, `caller`, ...). The full form takes `args` (read
by the worker that hosts the function), `filters`, `on`, `capture` and
`deliver`, and a policy:

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

### Hosting Observable Functions

The worker that hosts a function decides what observing it means with four
hooks. Each one overrides something iii does by default:

| Hook | What iii does by default |
|---|---|
| `on_observer_added(obs)` | accepts the observer: it becomes active and receives the function's calls. Raise `ObserveRejection` to reject it. |
| `on_observer_removed(obs)` | stops delivering to the observer and forgets it |
| `on_observer_match` | every observer wants every call |
| `on_observer_deliver(call, observers)` | runs the target of every matching observer whose `on` and filters match the call, and gives the caller the handler's return value |

`on_observer_added` and `on_observer_removed` can be set for one function or
for every function the worker hosts; both levels run, the worker's first when
an observer is added and last when it is removed. `on_observer_match` and
`on_observer_deliver` are set per function.

The methods work as decorators and as plain calls, on the client by function
id or on the `FunctionRef` that `register_function` returns. Set hooks by id
before registering the function so they run for every observer.

```python
from iii import Call, Observer, ObserveRejection

@iii.on_observer_added("cron::on-tick")
def start(obs: Observer) -> None:
    if not scheduler.is_valid(obs.args["expression"]):
        raise ObserveRejection("INVALID_CRON_EXPRESSION", "invalid expression")
    scheduler.retain(obs.args["expression"])

@iii.on_observer_removed("cron::on-tick")
def stop(obs: Observer) -> None:
    scheduler.release(obs.args["expression"])

# an observer of expression X gets the ticks of expression X
@iii.on_observer_match("cron::on-tick")
def same_expression(obs: Observer, call: Call) -> bool:
    return obs.args["expression"] == call.input["expression"]

iii.register_function(
    "cron::on-tick",
    lambda tick: tick,  # an event source returns the event it is called with
    args_format={"type": "object", "required": ["expression"],
                 "properties": {"expression": {"type": "string"}}},
    private=True,
)

# a tick is a call of the function
iii.call({"function_id": "cron::on-tick", "payload": {"expression": "*/5 * * * * *", "scheduled_time": now}})
```

`on_observer_match` takes a predicate `(obs, call) -> bool`, asked once per
active observer per call, or `observer=` and `call=` key functions for a
function with many observers. An observer wants a call when they share a key;
the SDK computes an observer's keys once, when it is accepted, and keeps an
index, so a call costs one lookup per key:

```python
iii.on_observer_match(
    "database::on-row-changed",
    observer=lambda obs: f"{obs.args['table']}|{obs.args['op']}",  # what this observer listens for
    call=lambda call: f"{call.input['table']}|{call.input['op']}",  # what this call is
)
```

iii's delivery applies the `on` and filters of the observers that matched.
`iii.is_observed(function_id, call)` tells a source whether a call would reach
anyone (on a function matched by keys, by looking up the call's keys), and
`iii.observers(function_id)` lists the active observers.

A function whose observers' answers matter, or whose calls each go to one
observer, delivers its calls itself. Its `on_observer_deliver` receives the
`Call` and the observers that matched it, calls through an observer, and
returns what the caller gets back. iii delivers nothing for that function:

```python
@iii.on_observer_deliver("http::on-request")
async def route(call, observers):
    obs = routes.match(call.input)
    if obs is None:
        return {"status_code": 404}
    if not obs.matches(call):
        return {"status_code": 422}
    return await iii.call_async({"observer": obs, "payload": call.input})
```

Inside any handler, `iii.current_invocation()` returns the running invocation
(`function_id`, `invocation_id`, `caller`, `activation`, `observe_depth`). Sync
handlers and hooks run on a thread of their own started in a copy of the
caller's context, so the current invocation and the OpenTelemetry context
follow them.

A function that sets `args_format`, `private=True`, `observable=False`, an
`on_observer_match` hook or an `on_observer_deliver` hook needs an engine that
advertises `observe.v2`; against an older engine it stays unregistered, and the
SDK logs `UNSUPPORTED` once for it.

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
