# Motia Framework for Python

High-level framework for building workflows with the III Engine.

## Installation

```bash
uv pip install motia
```

## Usage

### Defining a Step

```python
from motia import FlowContext, queue

config = {
    "name": "process-data",
    "triggers": [queue("data.created")],
    "enqueues": ["data.processed"],
}

async def handler(data: dict, ctx: FlowContext) -> None:
    ctx.logger.info("Processing data", data)
    await ctx.enqueue({"topic": "data.processed", "data": data})
```

### API Steps

```python
from motia import ApiRequest, ApiResponse, FlowContext, http

config = {
    "name": "create-item",
    "triggers": [http("POST", "/items")],
    "emits": ["item.created"],
}

async def handler(req: ApiRequest, ctx: FlowContext) -> ApiResponse:
    ctx.logger.info("Creating item", req.body)
    await ctx.enqueue({"topic": "item.created", "data": req.body})
    return ApiResponse(status=201, body={"id": "123"})
```

### Channel-based HTTP (Streaming)

```python
import json
from typing import Any

from motia import FlowContext, MotiaHttpArgs, http

config = {
    "name": "HttpChannelEcho",
    "triggers": [http("POST", "/http-channel/echo")],
}

async def handler(args: MotiaHttpArgs[Any], ctx: FlowContext[Any]) -> None:
    request = args.request
    response = args.response

    chunks: list[bytes] = []
    async for chunk in request.request_body.stream:
        chunks.append(chunk if isinstance(chunk, bytes) else str(chunk).encode("utf-8"))

    await response.status(200)
    await response.headers({"content-type": "application/json"})
    response.writer.stream.write(json.dumps({"receivedBytes": len(b"".join(chunks))}).encode("utf-8"))
    response.close()
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


### Build & Publish
```bash
python -m build
uv publish --index cloudsmith dist/*
```


## Features

- Event-driven step definitions
- API route handlers
- Cron job support
- Stream-based state management
- Type-safe context with logging

## Testing

1. Install dev dependencies:
   ```bash
   uv sync --extra dev
   ```

2. Run the unit test suite:
   ```bash
   uv run pytest -m "not integration"
   ```

3. Run the unit test suite with coverage:
   ```bash
   uv run pytest -m "not integration" --cov=src/motia --cov-report=term-missing
   ```

### Running Integration Tests

Integration tests require a running III Engine instance on the test ports below.

```bash
uv run pytest -m integration
```

### Test Configuration

Tests use non-default ports to avoid conflicts:
- Engine WebSocket: `ws://localhost:49199`
- HTTP API: `http://localhost:3199`

Set `III_ENGINE_PATH` environment variable to point to the III engine binary.

### Test Coverage

The coverage command above measures the unit-testable Python package code under `src/motia`.

The integration test suite covers:
- Bridge connection and function registration
- API triggers (HTTP endpoints)
- PubSub messaging
- Logging module
- Motia framework integration
- Stream operations (when available)
- State management (when available)
