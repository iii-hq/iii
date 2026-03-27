# Engine

For complete documentation on iii please visit [iii.dev/docs](https://iii.dev/docs).

## Install

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
```

This installs the iii engine (which includes all CLI commands).

<details>
<summary>Override install directory or pin a version</summary>

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | BIN_DIR=$HOME/.local/bin sh
```

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh -s -- v0.9.0
```

</details>

Verify:

```bash
command -v iii && iii --version
```

## Start the engine

```bash
iii --use-default-config
```

For a project-backed setup, create `config.yaml` in your working directory,
or run `iii --config /path/to/config.yaml`.

If you prefer a custom filename (for example `iii-config.yaml`), pass it explicitly:
`iii --config /path/to/iii-config.yaml`.

Open the console:

```bash
iii console
```

Your engine is running at `ws://localhost:49134` with HTTP API at `http://localhost:3111`.

Check out [iii.dev/docs](https://iii.dev/docs) to get started building with iii.

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
  -v ./iii-config.yaml:/app/iii-config.yaml:ro \
  iiidev/iii:latest
```

**Production (hardened)**

```bash
docker run --read-only --tmpfs /tmp \
  --cap-drop=ALL --cap-add=NET_BIND_SERVICE \
  --security-opt=no-new-privileges:true \
  -v ./iii-config.yaml:/app/iii-config.yaml:ro \
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

Visit [iii.dev/docs](https://iii.dev/docs) to learn how to [configure the engine](https://iii.dev/docs/how-to/configure-engine)

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
- `iii-config.yaml` – Example module configuration
- `examples/custom_queue_adapter.rs` – Custom module + adapter example

## Development

```bash
cargo run                                # start engine
cargo run -- --config iii-config.yaml        # with config
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
- [CLI & Engine](https://github.com/iii-hq/iii)
- [Console](https://github.com/iii-hq/console)
- [Examples](https://github.com/iii-hq/iii-examples)
- [SDKs](https://github.com/iii-hq/sdk)

## License

[Elastic License 2.0 (ELv2)](LICENSE)
