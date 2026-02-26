# iii

**A WebSocket-based process communication engine.**

[![License](https://img.shields.io/badge/license-ELv2-blue.svg)](LICENSE)
[![Docker](https://img.shields.io/docker/v/iiidev/iii?label=docker)](https://hub.docker.com/r/iiidev/iii)

Workers connect over WebSocket, register functions and triggers, and the engine routes invocations between them and its core modules. Core modules add HTTP APIs, queues, cron scheduling, state management, event streams, and observability.

## Three Primitives

| Primitive | What it does |
|-----------|-------------|
| **Worker** | A process that connects to the engine over WebSocket and runs your code |
| **Function** | A unit of work registered by a worker — receives input, returns output |
| **Trigger** | An event binding that invokes a function — HTTP route, cron schedule, queue topic, state change, or stream event |

## Quick Start

### 1. Install the engine

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
```

Specific version:
```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh -s -- v0.2.1
```

Override install directory:
```bash
BIN_DIR=/usr/local/bin curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
```

Verify:
```bash
command -v iii && iii --version
```

### 2. Start the engine

```bash
iii
```

The engine listens for workers at `ws://127.0.0.1:49134`.

### 3. Connect a worker

**Node.js**
```bash
npm install iii-sdk
```
```javascript
import { init } from 'iii-sdk'

const iii = init('ws://localhost:49134')

iii.registerFunction({ id: 'math.add' }, async (input) => {
  return { sum: input.a + input.b }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'math.add',
  config: { api_path: 'add', http_method: 'POST' },
})
```

**Python**
```bash
pip install iii-sdk
```
```python
from iii import III

iii = III("ws://localhost:49134")

async def add(data):
    return {"sum": data["a"] + data["b"]}

iii.register_function("math.add", add)

async def main():
    await iii.connect()

    iii.register_trigger(
        type="http",
        function_id="math.add",
        config={"api_path": "add", "http_method": "POST"}
    )
```

**Rust**
```rust
use iii_sdk::III;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii = III::new("ws://127.0.0.1:49134");
    iii.connect().await?;

    iii.register_function("math.add", |input| async move {
        let a = input.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
        let b = input.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
        Ok(json!({ "sum": a + b }))
    });

    iii.register_trigger("http", "math.add", json!({
        "api_path": "add",
        "http_method": "POST"
    }))?;

    Ok(())
}
```

Your function is now live at `http://localhost:3111/add`.

## Modules

| Module | Rust struct | What it does |
|--------|------------|-------------|
| HTTP | `RestApiModule` | Maps HTTP routes to functions via `http` triggers |
| Queue | `QueueModule` | Redis-backed publish/subscribe job queue |
| Cron | `CronModule` | Distributed cron scheduling with lock coordination |
| Stream | `StreamModule` | Real-time state sync over WebSocket |
| Observability | `OtelModule` | Structured logging, OpenTelemetry traces and metrics |
| Shell | `ExecModule` | File watcher that runs commands on change |

If `config.yaml` is missing, the engine loads defaults: HTTP, Queue, Cron, Stream, and Observability. Queue and Stream expect Redis at `redis://localhost:6379`.

## Docker

```bash
docker pull iiidev/iii:latest

docker run -p 3111:3111 -p 49134:49134 \
  -v ./config.yaml:/app/config.yaml:ro \
  iiidev/iii:latest
```

**Production (hardened)**
```bash
docker run --read-only --tmpfs /tmp \
  --cap-drop=ALL --cap-add=NET_BIND_SERVICE \
  --security-opt=no-new-privileges:true \
  -v ./config.yaml:/app/config.yaml:ro \
  -p 3111:3111 -p 49134:49134 -p 3112:3112 -p 9464:9464 \
  iiidev/iii:latest
```

**Docker Compose** (full stack with Redis + RabbitMQ):
```bash
docker compose up -d
```

**Docker Compose with Caddy** (TLS reverse proxy):
```bash
docker compose -f docker-compose.prod.yml up -d
```

See the [Caddy documentation](https://caddyserver.com/docs/) for TLS and reverse proxy configuration.

## Ports

| Port | Service |
|------|---------|
| 49134 | WebSocket (worker connections) |
| 3111 | HTTP API |
| 3112 | Stream API |
| 9464 | Prometheus metrics |

## SDKs

| Language | Package | Install |
|----------|---------|---------|
| Node.js | [`iii-sdk`](https://www.npmjs.com/package/iii-sdk) | `npm install iii-sdk` |
| Python | [`iii-sdk`](https://pypi.org/project/iii-sdk/) | `pip install iii-sdk` |
| Rust | [`iii-sdk`](https://crates.io/crates/iii-sdk) | Add to `Cargo.toml` |

## Configuration

Config files support environment expansion: `${REDIS_URL:redis://localhost:6379}`.

Minimal config (no Redis required):
```yaml
modules:
  - class: modules::api::RestApiModule
    config:
      host: 127.0.0.1
      port: 3111
  - class: modules::observability::OtelModule
    config:
      enabled: false
      level: info
      format: default
```

## Protocol Summary

The engine speaks JSON messages over WebSocket. Key message types:
`registerfunction`, `invokefunction`, `invocationresult`,
`registertrigger`, `unregistertrigger`, `triggerregistrationresult`, `registerservice`,
`functionsavailable`, `ping`, `pong`.

Invocations can be fire-and-forget by omitting `invocation_id`.

## Repository Layout

- `src/main.rs` – CLI entrypoint (`iii` binary)
- `src/engine/` – Worker management, routing, and invocation lifecycle
- `src/protocol.rs` – WebSocket message schema
- `src/modules/` – Core modules (API, queue, cron, stream, observability, shell)
- `config.yaml` – Example module configuration
- `examples/custom_queue_adapter.rs` – Custom module + adapter example

## Development

```bash
cargo run                                # start engine
cargo run -- --config config.yaml        # with config
cargo fmt && cargo clippy -- -D warnings # lint
make watch                               # watch mode
```

### Building Docker images locally

```bash
docker build -t iii:local .                        # production (distroless)
docker build -f Dockerfile.debug -t iii:debug .    # debug (Debian + shell)
```

The Docker images include distroless runtime (no shell, minimal attack surface), non-root user execution, Trivy vulnerability scanning in CI, SBOM attestation, and build provenance.

## Resources

- [Documentation](https://iii.dev/docs)
- [Examples](https://github.com/iii-hq/iii-examples)
- [Console](https://github.com/iii-hq/console)
- [SDKs](https://github.com/iii-hq/sdk)

## License

[Elastic License 2.0 (ELv2)](LICENSE)
