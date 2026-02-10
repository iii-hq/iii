# III Engine

III is a WebSocket-based process communication engine. Workers connect over WS, register
functions and triggers, and the engine routes invocations between workers and core modules.
Core modules add HTTP APIs, event stream, cron scheduling, and logging.

## Quick Start

Prerequisites:

- Rust 1.80+ (edition 2024)
- Redis (only if you enable the event/cron/stream modules; the default config expects Redis at
  `redis://localhost:6379`). Cron uses the built-in KV adapter by default.

Install (prebuilt binary)
-------------------------
This installer currently supports macOS and Linux (not native Windows).
You can install the latest release binary with:
```bash
curl -fsSL https://raw.githubusercontent.com/MotiaDev/iii-engine/main/install.sh | sh
```

To install a specific version, pass it as the first argument (the leading `v` is optional):
```bash
curl -fsSL https://raw.githubusercontent.com/MotiaDev/iii-engine/main/install.sh | sh -s -- v0.2.1
```
Or set `VERSION` explicitly:
```bash
VERSION=0.2.1 curl -fsSL https://raw.githubusercontent.com/MotiaDev/iii-engine/main/install.sh | sh
```

By default, the binary is installed to `~/.local/bin`. Override with `BIN_DIR` or `PREFIX`:
```bash
BIN_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/MotiaDev/iii-engine/main/install.sh | sh
```

To check that the binary is on your PATH and see the current version:
```bash
command -v iii && iii --version
```

Docker
------

Pull the pre-built image:
```bash
docker pull iiidev/iii:latest
```

Run with a config file:
```bash
docker run -p 3111:3111 -p 49134:49134 \
  -v ./config.yaml:/app/config.yaml:ro \
  iiidev/iii:latest
```

**Production (hardened)**:
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

| Port | Service |
|------|---------|
| 49134 | WebSocket (worker connections) |
| 3111 | REST API |
| 3112 | Stream API |
| 9464 | Prometheus metrics |

Run the engine:

```bash
cargo run
# or explicitly pass a config
cargo run -- --config config.yaml
```

The engine listens for workers at `ws://127.0.0.1:49134`.

If you want to run without Redis, create a minimal config that only loads modules you need:

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

Config files support environment expansion like `${REDIS_URL:redis://localhost:6379}`.

## Connect a Worker

Node.js (SDK in `packages/node/iii`):

```javascript
import { Bridge } from '@iii-dev/sdk'

const bridge = new Bridge('ws://127.0.0.1:49134')

bridge.registerFunction({ function_id: 'math.add' }, async (input) => {
  return { sum: input.a + input.b }
})
```

Rust (SDK in `packages/rust/iii`):

```rust
use iii_sdk::Bridge;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  let bridge = Bridge::new("ws://127.0.0.1:49134");
  bridge.connect().await?;

  bridge.register_function("math.add", |input| async move {
    let a = input.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
    let b = input.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
    Ok(json!({ "sum": a + b }))
  });

  Ok(())
}
```

## Expose an HTTP Endpoint (API trigger)

The REST API module maps HTTP routes to functions via the `api` trigger type. Functions should
return `{ "status_code": <int>, "body": <json> }`.

```javascript
bridge.registerFunction({ function_id: 'api.echo' }, async (req) => {
  return { status_code: 200, body: { ok: true, input: req.body } }
})

bridge.registerTrigger({
  trigger_type: 'api',
  function_id: 'api.echo',
  config: { api_path: 'echo', http_method: 'POST' },
})
```

With the default API config, the endpoint will be available at:
`http://127.0.0.1:3111/echo`.

## Modules

Available core modules (registered in `src/modules/config.rs`):

- `modules::api::RestApiModule` – HTTP API trigger (`api`) on `host:port` (default `127.0.0.1:3111`).
- `modules::queue::QueueModule` – Redis-backed queue system (`queue` trigger, `emit` function).
- `modules::cron::CronModule` – Cron-based scheduling (`cron` trigger, built-in KV adapter by default).
- `modules::stream::StreamModule` – Stream WebSocket API (default `127.0.0.1:3112`) and
  `stream.set/get/delete/list` functions (Redis-backed by default).
- `modules::observability::OtelModule` – Observability: `log.info/warn/error/debug`, traces, metrics, and alerts.
- `modules::shell::ExecModule` – File watcher that runs commands (only when configured).

If `config.yaml` is missing, the engine loads the default module list:
RestApi, Queue, Logging, Cron, Stream. Queue/Stream expect Redis; Cron uses built-in KV by default.

## Protocol Summary

The engine speaks JSON messages over WebSocket. Key message types:
`registerfunction`, `invokefunction`, `invocationresult`, `registertrigger_type`,
`registertrigger`, `unregistertrigger`, `triggerregistrationresult`, `registerservice`,
`functionsavailable`, `ping`, `pong`.

Invocations can be fire-and-forget by omitting `invocation_id`.

## Repository Layout

- `src/main.rs` – CLI entrypoint (`iii` binary).
- `src/engine/` – Worker management, routing, and invocation lifecycle.
- `src/protocol.rs` – WebSocket message schema.
- `src/modules/` – Core modules (API, event, cron, stream, logging, shell).
- `config.yaml` – Example module configuration.
- `packages/node/*` and `packages/rust/*` – SDKs and higher-level frameworks.
- `examples/custom_queue_adapter.rs` – Example of a custom module + adapter.

## Development

- Format/lint: `cargo fmt && cargo clippy -- -D warnings`
- Watch run: `make watch` (or `make watch-debug` for verbose logs)

### Building Docker Images Locally

```bash
# Production image (distroless runtime)
docker build -t iii:local .

# Debug image (Debian with shell, htop, vim)
docker build -f Dockerfile.debug -t iii:debug .

# Run locally built image
docker run --rm iii:local --version
```

### Security

The Docker images include:
- Distroless runtime (no shell, minimal attack surface)
- Non-root user execution
- Trivy vulnerability scanning in CI
- SBOM (Software Bill of Materials) attestation
- Build provenance

For production deployments, always use the hardened runtime flags documented above.
