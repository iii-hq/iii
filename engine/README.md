# Engine

The iii engine provides durable orchestration, interoperable cross-language execution, live
discovery of functionality, live system extensibility, and live system observability from three
simple primitives: Function, Trigger, and Worker.

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
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh -s -- v0.11.3
```

</details>

Verify:

```bash
command -v iii && iii --version
```

## Start the engine

```bash
iii
```

With no `config.yaml` in the working directory, this offers to create one and then starts the
engine. Headless runs, such as a container or a CI job, create it without prompting.

For a project-backed setup, create `config.yaml` in your working directory, or run
`iii --config /path/to/config.yaml`.

If you prefer a custom filename (for example `iii-config.yaml`), pass it explicitly:
`iii --config /path/to/iii-config.yaml`.

Open the console: add the ADE worker to your project, then open `http://127.0.0.1:3113`:

```bash
iii trigger compose::add worker=ade
```

Your engine is running at `ws://localhost:49134`. Project HTTP routes are served by the `http`
worker on `http://localhost:3111`; add it the same way with
`iii trigger compose::add worker=http`.

`compose::add` is served by the Compose daemon, not by the engine, so both commands need one
running: start it with `iii compose` in another terminal, or pass `-n <namespace>` to reach an
existing daemon. Not `iii compose --up` here — that also starts the engine declared in
`worker-compose.yaml`, and this section has already started one on the same endpoint.

Check out [iii.dev/docs](https://iii.dev/docs) to get started building with iii.

## SDKs

| Language | Package                                            | Install                                     |
| -------- | -------------------------------------------------- | ------------------------------------------- |
| Node.js  | [`iii-sdk`](https://www.npmjs.com/package/iii-sdk) | `pnpm add iii-sdk` or `npm install iii-sdk` |
| Python   | [`iii-sdk`](https://pypi.org/project/iii-sdk/)     | `pip install iii-sdk`                       |
| Rust     | [`iii-sdk`](https://crates.io/crates/iii-sdk)      | Add to `Cargo.toml`                         |

## Docker

Run these from `engine/`, where `config.yaml` and the compose files live. From the repository root,
hand compose the file explicitly: `docker compose -f engine/docker-compose.yml up -d`.

```bash
docker pull iiidev/iii:latest

docker run -p 3111:3111 -p 49134:49134 \
  -v ./config.yaml:/app/config.yaml:ro \
  iiidev/iii:latest
```

The image runs the engine only. `49134` is the SDK socket; `3111` stays closed until a project adds
the `http` worker, which a bare container does not start.

The `docker run` commands below mount a `config.yaml` from the current directory. If you mount the
repository's `engine/config.yaml`, it enables the deprecated `iii-stream` worker with
`redis://localhost:6379`, so the container also needs a reachable Redis: put both on one Docker
network and set `redis_url` to the Redis service name, or drop the `iii-stream` entry.

That config also pins `iii-stream` to `host: 127.0.0.1`, so the published `3112` port is not
reachable from outside the container. Change it to `0.0.0.0` to use the stream API through Docker,
or stop publishing `3112`.

### Production Example

```bash
docker run --read-only --tmpfs /tmp \
  --cap-drop=ALL --cap-add=NET_BIND_SERVICE \
  --security-opt=no-new-privileges:true \
  -v ./config.yaml:/app/config.yaml:ro \
  -v /app/config -v /app/data \
  -p 3111:3111 -p 49134:49134 -p 3112:3112 -p 9464:9464 \
  iiidev/iii:latest
```

`/app/config` and `/app/data` stay writable through anonymous volumes, which inherit the image's
`65532` ownership: the `configuration` worker stores one YAML per entry under `/app/config` and
observability traces default to `/app/data`. They last as long as the container; use named volumes
or host paths when they should survive a recreate.

### Docker Compose (full stack with Redis + RabbitMQ):

```bash
docker compose up -d
```

### Docker Compose with Caddy (TLS reverse proxy):

```bash
docker compose -f docker-compose.prod.yml up -d
```

See the [Caddy documentation](https://caddyserver.com/docs/) for TLS and reverse proxy
configuration.

## Ports

| Port  | Service                            | Bound by                   |
| ----- | ---------------------------------- | -------------------------- |
| 49134 | SDK WebSocket (worker connections) | engine                     |
| 3111  | Project HTTP routes                | `http` worker              |
| 3112  | Stream API (deprecated)            | engine (`iii-stream`)      |
| 3113  | Console UI and `/ws` proxy         | `ade` worker               |
| 9464  | Prometheus metrics                 | `iii-observability` worker |

The engine process itself binds the SDK WebSocket, the deprecated stream listener, and nothing else.
The other surfaces belong to workers: engine-owned (`iii-stream`), injected automatically
(`iii-observability`, `iii-telemetry`, `iii-engine-functions`), or Compose-declared. See the
[engine protocol reference](https://iii.dev/docs/reference/engine-protocol) for the canonical table.

## Configuration

See [Engine](https://iii.dev/docs/using-iii/engine) for startup and engine-owned worker
configuration, and [Configuration](https://iii.dev/docs/using-iii/configuration) for per-worker
settings.

## Protocol Summary

The engine speaks JSON messages over WebSocket. Key message types: `registerfunction`,
`invokefunction`, `invocationresult`, `registertrigger`, `unregistertrigger`,
`triggerregistrationresult`, `registerservice`, `functionsavailable`, `ping`, `pong`.

Invocations can be fire-and-forget by omitting `invocation_id`. Observers (`iii.observe`) added
`Observe` and `Unobserve` frames in 0.25. The [engine protocol
reference](https://iii.dev/docs/reference/engine-protocol) is the canonical message list.

## Repository Layout

- `src/main.rs` – CLI entrypoint (`iii` binary)
- `src/cli/` – CLI subcommands (`compose`, `trigger`, `project`, `cloud`, `update`)
- `src/engine/` – Worker registry, routing, and invocation lifecycle
- `src/invocation/` – Invocation state and result handling
- `src/observer/` – Observer registry, filters, and activations
- `src/workers/` – Engine-owned workers (stream, configuration, http functions, sandbox, worker
  manager, observability, telemetry) and queue adapters
- `src/builtins/` – In-process KV, queue, and pub/sub backends
- `src/protocol.rs` – WebSocket message schema
- `config.yaml` – Example engine and engine-owned worker configuration
- `examples/custom_queue_adapter.rs` – Custom worker and adapter example

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

Docker image security: distroless runtime (no shell), non-root execution, Trivy scanning in CI, SBOM
attestation, and build provenance.

## Examples

See the [Quickstart guide](https://iii.dev/docs/quickstart) for step-by-step tutorials.

## Resources

- [Documentation](https://iii.dev/docs)
- [Worker registry](https://workers.iii.dev)
- [CLI & Engine](https://github.com/iii-hq/iii)
- [Console](https://iii.dev/docs/using-iii/console)
- [Examples](https://github.com/iii-hq/iii-examples)
- [SDKs](https://github.com/iii-hq/iii/tree/main/sdk)

## License

The engine and CLI are licensed under the [Elastic License 2.0 (ELv2)](LICENSE).
