# Motia Framework for Python

High-level framework for building workflows with the III Engine.

## Installation

```bash
pip install iii-motia
```

## Usage

### Defining a Step

```python
from motia import step_wrapper, EventConfig, FlowContext

config = EventConfig(
    type="event",
    name="process-data",
    subscribes=["data.created"],
    emits=["data.processed"],
)

async def handler(data: dict, ctx: FlowContext) -> None:
    ctx.logger.info("Processing data", data)
    await ctx.emit({"topic": "data.processed", "data": data})

step_wrapper(config, __file__, handler)
```

### API Steps

```python
from motia import step_wrapper, ApiRouteConfig, ApiRequest, ApiResponse, FlowContext

config = ApiRouteConfig(
    type="api",
    name="create-item",
    path="/items",
    method="POST",
    emits=["item.created"],
)

async def handler(req: ApiRequest, ctx: FlowContext) -> ApiResponse:
    ctx.logger.info("Creating item", req.body)
    await ctx.emit({"topic": "item.created", "data": req.body})
    return ApiResponse(status=201, body={"id": "123"})

step_wrapper(config, __file__, handler)
```

### Streams

```python
from motia import Stream

# Define a stream
todo_stream = Stream[dict]("todos")

# Use the stream
item = await todo_stream.get("group-1", "item-1")
await todo_stream.set("group-1", "item-1", {"title": "Buy milk"})
await todo_stream.delete("group-1", "item-1")
items = await todo_stream.get_group("group-1")
```

## Features

- Event-driven step definitions
- API route handlers
- Cron job support
- Stream-based state management
- Type-safe context with logging
