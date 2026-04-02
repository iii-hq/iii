# Motia Example

This example demonstrates the Motia framework with a Todo application workflow.

## Quick Start

### 1. Install uv

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

### 2. Enter the project folder

```bash
cd playground
```

### 3. install the dependencies.

```bash
uv sync --all-extras
```

### 4. Run the example

```bash
uv run motia run --dir steps
```

## Project Structure

```
playground/
├── pyproject.toml              # Project configuration with dependencies
├── uv.lock                     # Locked dependency versions
├── README.md                   # This file
└── steps/                      # Example workflows and triggers
    ├── __init__.py
    ├── api_steps/              # HTTP API examples (SSE + HTTP channels)
    ├── conditions/             # Conditional trigger examples
    ├── greetings/              # API CRUD + summary flow examples
    ├── multi_trigger/          # Single/multi trigger examples (API/Event/Cron)
    ├── otel_example/           # OpenTelemetry instrumentation example
    ├── state_example/          # State update + state-change listener examples
    ├── stream_example/         # Stream-based todo event example
    └── todo/                   # Todo CRUD workflow examples
```

## What This Example Does

This example showcases Motia's event-driven workflow capabilities with a simple Todo application:

- **Event Steps**: React to events like `todo.created`, `todo.updated`, `todo.deleted`
- **API Steps**: Expose REST endpoints for CRUD operations
- **HTTP Channels**: Read request body streams and stream custom responses
- **Stream State**: Manage todo items using Motia's stream-based state management
- **Multi-trigger Steps**: Handle multiple event types in a single step
- **Conditional Logic**: Demonstrate conditional step execution

## HTTP Channel Example

The `steps/api_steps/http_channel_echo_step.py` step demonstrates channel-based
HTTP handling with `MotiaHttpArgs`:

- Reads request chunks from `request.request_body.stream`
- Streams response bytes with `response.writer.stream`

Try it with:

```bash
curl -X POST http://localhost:3000/http-channel/echo \
  -H "content-type: application/json" \
  -d '{"message":"hello channel","items":[1,2,3]}'
```

## Documentation

For more information on the Motia framework, see:

- [Motia README](../motia/README.md)
- [III SDK README](../iii/README.md)
