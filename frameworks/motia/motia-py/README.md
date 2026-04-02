# Python Packages

This directory contains Python packages for the III Engine.

## Quick Start

```bash
# 1) Create and activate a virtual environment
python -m venv .venv
source .venv/bin/activate

# 2) Create requirements.txt
cat > requirements.txt << 'EOF'
motia
iii-sdk==0.2.0
EOF

# 3) Install dependencies
pip install -r requirements.txt

# 4) Create steps folder and a *_steps.py file
mkdir -p steps
cat > steps/single_event_steps.py << 'EOF'
from typing import Any

from motia import FlowContext, queue

config = {
    "name": "SingleEventTrigger",
    "description": "Test single event trigger",
    "triggers": [queue("test.event")],
    "enqueues": ["test.processed"],
}


async def handler(input: Any, ctx: FlowContext[Any]) -> None:
    ctx.logger.info("Single event trigger fired", {"data": input})
EOF

# 5) Run Motia (inside the active venv)
motia run
```

In another terminal, start III:

```bash
iii
```

## Packages

### iii

The core SDK for communicating with the III Engine via WebSocket.

```bash
cd iii
uv pip install -e .
```

### motia

High-level framework for building workflows with the III Engine.

```bash
cd motia
uv pip install -e .
```

## Examples

### iii-example

Basic example demonstrating the III SDK.

```bash
cd iii-example
uv pip install -e ../iii
python src/main.py
```

### motia-example

Example demonstrating the Motia framework with a Todo application.

```bash
cd motia-example
uv pip install -e ../iii -e ../motia
motia run --dir steps
```

## Development

### Install all packages in development mode

```bash
uv pip install -e iii -e motia
```

### Run tests

```bash
cd packages/motia
uv sync --extra dev
uv run pytest -m "not integration"
uv run pytest -m "not integration" --cov=src/motia --cov-report=term-missing
```

### Type checking

```bash
cd iii && mypy src
cd motia && mypy src
```

### Linting

```bash
cd iii && ruff check src
cd motia && ruff check src
```
