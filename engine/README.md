# iii engine

[![Release](https://img.shields.io/github/v/release/iii-hq/iii?label=release)](https://github.com/iii-hq/iii/releases)
[![Docker pulls](https://img.shields.io/docker/pulls/iiidev/iii?logo=docker&logoColor=white)](https://hub.docker.com/r/iiidev/iii)
[![License: ELv2](https://img.shields.io/badge/license-ELv2-blue)](LICENSE)

The engine is the runtime at the center of an iii system, and the `iii` CLI. Workers connect to it over WebSocket. The engine routes every function call between them. It is written in Rust.

## What the engine does

- **Live registry.** The engine records each connected worker, the functions it registers, and the observers it creates. Workers read this registry or observe its changes.
- **Routing.** When a function is called, the engine finds a worker that provides it and sends the call there. When a function runs, the engine runs its observers. The routing is the same for every language, runtime, and location.
- **Disconnect cleanup.** When a worker disconnects, the engine removes its functions and cancels their in-flight calls. The rest of the system continues to operate.
- **Hot reload.** The engine watches its config file. When the file changes, it restarts only the workers that changed.

## Install

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
```

Or run the Docker image:

```bash
docker pull iiidev/iii
```

The [install guide](https://iii.dev/docs/install) has all installer options.

## CLI

| Command | Purpose |
| --- | --- |
| `iii` | Start the engine with `config.yaml` |
| `iii project init <name>` | Create a project |
| `iii compose --up` | Start the engine and the workers in `worker-compose.yaml` |
| `iii call <function> key=value` | Call a function on the running engine |
| `iii call compose::add worker=<name>` | Add a worker from the registry to the project |
| `iii ps` / `iii kill` | List or stop iii processes on this machine |
| `iii update` | Update iii and its managed binaries |
| `iii cloud` | Manage iii Cloud deployments |

Run `iii --help` or `iii <command> --help` for all flags. The full reference is at [iii.dev/docs](https://iii.dev/docs).

## Engine workers

The engine contains these workers. Each one has a README in [`src/workers/`](src/workers/).

| Worker | Purpose |
| --- | --- |
| `iii-worker-manager` | WebSocket listeners for SDK workers. The default port is `49134`. |
| `configuration` | Schema-validated configuration entries for workers |
| `iii-engine-functions` | The `engine::*` functions that read the live registry |
| `iii-observability` | OpenTelemetry traces, logs, and metrics |
| `iii-sandbox` | `sandbox::*` functions that run code in microVMs |
| `iii-http-functions` | Functions served by external HTTP endpoints |
| `iii-stream` | Deprecated. See the [migration guide](https://iii.dev/docs/upgrading/migrate-from-streams). |

All other capabilities (HTTP, cron, queues, state, agents) are workers from the [worker registry](https://iii.dev/registry).

## Build from source

Run from the repository root:

```bash
cargo build --release -p iii     # build the engine
cargo run --release -p iii       # start the engine with engine/config.yaml
cargo test -p iii                # run the engine tests
cargo bench -p iii               # run the benchmarks in benches/
```

This directory also has a [`Dockerfile`](Dockerfile) and [`docker-compose.yml`](docker-compose.yml).

See [CONTRIBUTING.md](CONTRIBUTING.md) for code style and license headers.

## License

The engine and CLI are licensed under the [Elastic License 2.0](LICENSE). Contributions are made under Apache 2.0. See the [root CONTRIBUTING.md](../CONTRIBUTING.md#license-of-contributions).
