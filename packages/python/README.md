# Python Packages

This directory contains Python packages for the III Engine.

## Packages

### iii

The core SDK for communicating with the III Engine via WebSocket.

```bash
cd iii
pip install -e .
```

### motia

High-level framework for building workflows with the III Engine.

```bash
cd motia
pip install -e .
```

## Examples

### iii-example

Basic example demonstrating the III SDK.

```bash
cd iii-example
pip install -e ../iii
python src/main.py
```

### motia-example

Example demonstrating the Motia framework with a Todo application.

```bash
cd motia-example
pip install -e ../iii -e ../motia
motia run --dir steps
```

## Development

### Install all packages in development mode

```bash
pip install -e iii -e motia
```

### Run tests

```bash
cd iii && pytest
cd motia && pytest
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
