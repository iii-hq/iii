![iii - One Engine, Three Primitives](assets/banner.jpg)

[![License](https://img.shields.io/badge/license-ELv2-blue.svg)](LICENSE)
[![Docker](https://img.shields.io/docker/v/iiidev/iii?label=docker)](https://hub.docker.com/r/iiidev/iii)
[![npm](https://img.shields.io/npm/v/iii-sdk?label=npm)](https://www.npmjs.com/package/iii-sdk)
[![PyPI](https://img.shields.io/pypi/v/iii-sdk?label=pypi)](https://pypi.org/project/iii-sdk/)
[![Crates.io](https://img.shields.io/crates/v/iii-sdk?label=crates.io)](https://crates.io/crates/iii-sdk)

## What is iii

You start building a backend and immediately need six different tools: an API framework, a task queue, a cron scheduler, pub/sub, a state store, and an observability pipeline. Each has its own config, its own deployment, its own failure modes. A simple "process this, then notify that" workflow touches three services before you write any business logic.

iii replaces all of that with a single engine and three primitives: **Function**, **Trigger**, and **Worker**.

A Function is anything that does work. A Trigger is what causes it to run — an HTTP request, a cron schedule, a queue message, a state change. A Worker is any process that registers Functions and Triggers — long-running services, ephemeral scripts, agentic workers, or legacy systems via middleware. You write the function, declare what triggers it, connect a worker, and the engine handles routing, retries, and observability.

One config file. One process. Everything discoverable. Think of it the way React gave frontend a single model for UI — iii gives your backend a single model for execution.

## Three Primitives

| Primitive     | What it does |
| ------------- | ------------ |
| **Function**  | A unit of work. It receives input and optionally returns output. It can exist anywhere: locally, in the cloud, on serverless, or as a third-party HTTP endpoint. |
| **Trigger**   | What causes a Function to run — explicitly from code, or automatically from an event source. Examples: HTTP route, cron schedule, queue topic, state change, stream event. |
| **Worker**    | Any process that registers Functions and Triggers. Long-running services, ephemeral scripts, agentic workers, or legacy systems via middleware — all connect and participate as first-class members. |

## Quick Start

### Install

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
```

This installs both the engine and iii-cli.

<details>
<summary>Override install directory or pin a version</summary>

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | BIN_DIR=$HOME/.local/bin sh
```

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh -s -- v0.7.0
```

</details>

Verify:

```bash
command -v iii && iii --version
```

### Start the engine

```bash
iii-cli start --use-default-config
```

For a project-backed setup, create `config.yaml` in your working directory or run `iii-cli start --config /path/to/config.yaml`.

Open the console:

```bash
iii-cli console
```

Your engine is running at `ws://localhost:49134` with HTTP API at `http://localhost:3111`.

## Connect a Worker

### Node.js

```bash
npm install iii-sdk
```

```javascript
import { registerWorker } from 'iii-sdk';

const iii = registerWorker('ws://localhost:49134');

iii.registerFunction({ id: 'math.add' }, async (input) => {
  return { sum: input.a + input.b };
});

iii.registerTrigger({
  type: 'http',
  function_id: 'math.add',
  config: { api_path: 'add', http_method: 'POST' },
});
```

<details>
<summary>Python</summary>

```bash
pip install iii-sdk
```

```python
from iii import register_worker

iii = register_worker("ws://localhost:49134")

def add(data):
    return {"sum": data["a"] + data["b"]}

iii.register_function({"id": "math.add"}, add)

iii.register_trigger({
    "type": "http",
    "function_id": "math.add",
    "config": {"api_path": "add", "http_method": "POST"}
})
```

</details>

<details>
<summary>Rust</summary>

```rust
use iii_sdk::{register_worker, InitOptions, RegisterFunctionMessage, RegisterTriggerInput};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default())?;

    iii.register_function(RegisterFunctionMessage { id: "math.add".into(), description: None, request_format: None, response_format: None, metadata: None, invocation: None }, |input| async move {
        let a = input.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
        let b = input.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
        Ok(json!({ "sum": a + b }))
    });

    iii.register_trigger(RegisterTriggerInput { trigger_type: "http".into(), function_id: "math.add".into(), config: json!({
        "api_path": "add",
        "http_method": "POST"
    }) })?;

    Ok(())
}
```

</details>

Your function is now live at `http://localhost:3111/add`.

## Console

The [iii-console](https://github.com/iii-hq/console) is a developer and operations dashboard for inspecting functions, triggers, traces, and real-time state. Launch it with:

```bash
iii-cli console
```

![iii console dashboard](https://raw.githubusercontent.com/iii-hq/docs/main/public/docs/console/dashboard-dark.png)

## Modules

| Module         | Class                  | What it does                                                      | Default |
| -------------- | ---------------------- | ----------------------------------------------------------------- | ------- |
| HTTP API       | `RestApiModule`        | Maps HTTP routes to functions via `http` triggers with CORS       | Yes     |
| Queue          | `QueueModule`          | Message queue with pluggable adapters (built-in, Redis, RabbitMQ) | Yes     |
| Cron           | `CronModule`           | Scheduled job execution with cron expression triggers             | Yes     |
| Stream         | `StreamModule`         | Real-time bidirectional streaming over WebSocket                  | Yes     |
| Pub/Sub        | `PubSubModule`         | Topic-based event publishing and subscription                     | Yes     |
| State          | `StateModule`          | Distributed state management with get/set/delete and state triggers | Yes     |
| HTTP Functions | `HttpFunctionsModule`  | Proxy for invoking external HTTP endpoints as functions            | No      |
| Observability  | `OtelModule`           | OpenTelemetry traces, metrics, and logs with OTLP export          | No      |
| Shell          | `ExecModule`           | File watcher that runs shell commands on change                   | No      |

To run with built-in defaults, start the engine with `--use-default-config`. Otherwise the engine expects `config.yaml` (or a path passed with `--config`) and exits if the file is missing. Queue and Stream use their built-in adapters by default; switch to Redis or RabbitMQ in `config.yaml` for production.

## SDKs

| Language | Package                                            | Install               |
| -------- | -------------------------------------------------- | --------------------- |
| Node.js  | [`iii-sdk`](https://www.npmjs.com/package/iii-sdk) | `npm install iii-sdk` |
| Python   | [`iii-sdk`](https://pypi.org/project/iii-sdk/)     | `pip install iii-sdk` |
| Rust     | [`iii-sdk`](https://crates.io/crates/iii-sdk)      | Add to `Cargo.toml`   |

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

| Port  | Service                        |
| ----- | ------------------------------ |
| 49134 | WebSocket (worker connections) |
| 3111  | HTTP API                       |
| 3112  | Stream API                     |
| 9464  | Prometheus metrics             |

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

Docker image security: distroless runtime (no shell), non-root execution, Trivy scanning in CI, SBOM attestation, and build provenance.

## Examples

See the [Quickstart guide](https://iii.dev/docs/quickstart) for step-by-step tutorials.

## Resources

- [Documentation](https://iii.dev/docs)
- [CLI](https://github.com/iii-hq/iii-cli)
- [Console](https://github.com/iii-hq/console)
- [Examples](https://github.com/iii-hq/iii-examples)
- [SDKs](https://github.com/iii-hq/sdk)

## License

[Elastic License 2.0 (ELv2)](LICENSE)
