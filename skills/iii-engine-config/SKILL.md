---
name: iii-engine-config
description: >-
  Configures the iii engine via iii-config.yaml — modules, adapters, queue
  configs, ports, and environment variables. Use when deploying, tuning, or
  customizing the engine.
---

# Engine Config

Comparable to: Infrastructure as code, Docker Compose configs

## Key Concepts

Use the concepts below when they fit the task. Not every deployment needs all modules or adapters.

- **iii-config.yaml** defines the engine port, modules, workers, adapters, and queue configs
- **Environment variables** use `${VAR:default}` syntax (default is optional)
- **Modules** are the building blocks — each enables a capability (API, state, queue, cron, etc.)
- **Workers** are external binary modules managed via `iii.toml` and the `iii worker` CLI commands
- **Adapters** swap storage backends per module: in_memory, file_based, Redis, RabbitMQ
- **Queue configs** control retry count, concurrency, ordering, and backoff per named queue
- The engine listens on port **49134** (WebSocket) for SDK/worker connections

## Architecture

The iii-config.yaml file is loaded by the iii engine binary at startup. Modules are initialized in order, adapters connect to their backends, and the engine begins accepting worker connections over WebSocket on port 49134. External workers defined in the `workers` section are spawned as child processes automatically.

## iii Primitives Used

| Primitive                                      | Purpose                                |
| ---------------------------------------------- | -------------------------------------- |
| `modules::api::RestApiModule`                  | HTTP API server (port 3111)            |
| `modules::stream::StreamModule`                | WebSocket streams (port 3112)          |
| `modules::state::StateModule`                  | Persistent key-value state storage     |
| `modules::queue::QueueModule`                  | Background job processing with retries |
| `modules::pubsub::PubSubModule`                | In-process event fanout                |
| `modules::cron::CronModule`                    | Time-based scheduling                  |
| `modules::observability::OtelModule`           | OpenTelemetry traces, metrics, logs    |
| `modules::http_functions::HttpFunctionsModule` | Outbound HTTP call security            |
| `modules::shell::ExecModule`                   | Spawn external processes               |
| `modules::bridge_client::BridgeClientModule`   | Distributed cross-engine invocation    |
| `modules::telemetry::TelemetryModule`          | Anonymous product analytics            |
| `workers` section in iii-config.yaml               | External binary workers (worker modules)|
| `iii.toml`                                     | Worker manifest (name → version)       |
| `iii worker add NAME[@VERSION]`                | Install a worker from the registry     |
| `iii worker remove NAME`                       | Uninstall a worker                     |
| `iii worker list`                              | List installed workers                 |
| `iii worker info NAME`                         | Show registry info for a worker        |

## Reference Implementation

See [../references/iii-config.yaml](../references/iii-config.yaml) for the full working example — a complete
engine configuration with all modules, adapters, queue configs, and environment variable patterns.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `iii --config ./iii-config.yaml` — start the engine with a config file
- `docker pull iiidev/iii:latest` — pull the Docker image
- Dev storage: `store_method: file_based` with `file_path: ./data/...`
- Prod storage: Redis adapters with `redis_url: ${REDIS_URL}`
- Prod queues: RabbitMQ adapter with `amqp_url: ${AMQP_URL}` and `queue_mode: quorum`
- Queue config: `queue_configs` with `max_retries`, `concurrency`, `type`, `backoff_ms` per queue name
- Env var with fallback: `port: ${III_PORT:49134}`
- Health check: `curl http://localhost:3111/health`
- Ports: 3111 (API), 3112 (streams), 49134 (engine WS), 9464 (Prometheus)

### Worker Module System

External workers are installed via the CLI and configured in `iii-config.yaml`:

- `iii worker add pdfkit@1.0.0` — install a worker binary from the registry
- `iii worker add` (no name) — install all workers listed in `iii.toml`
- `iii worker remove pdfkit` — remove binary, manifest entry, and config block
- `iii worker list` — show installed workers and versions from `iii.toml`

Workers appear in `iii.toml` as a version manifest:
```toml
[workers]
pdfkit = "1.0.0"
image-processor = "2.3.1"
```

Worker config blocks in `iii-config.yaml` use marker comments for automatic management:
```yaml
workers:
  # === iii:pdfkit BEGIN ===
  - class: workers::pdfkit::PdfKitWorker
    config:
      output_dir: ./output
  # === iii:pdfkit END ===
```

At startup, the engine resolves each worker class, finds the binary in `iii_workers/`, and spawns it as a child process. Worker binaries are stored in the `iii_workers/` directory.

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Start with file_based adapters for development, switch to Redis/RabbitMQ for production
- Define queue configs per workload: high-concurrency for parallel jobs, FIFO for ordered processing
- Use environment variables with defaults for all deployment-sensitive values (URLs, ports, credentials)
- Enable only the modules you need — unused modules can be omitted from the config
- Use `iii worker add` to install external workers and auto-generate their config blocks
- Set `max_retries` and `backoff_ms` based on your failure tolerance and SLA requirements
- Configure `OtelModule` with your collector endpoint and sampling ratio for observability

## Pattern Boundaries

- For HTTP handler logic (request/response, path params), prefer `iii-http-endpoints`.
- For queue processing patterns (enqueue, FIFO, concurrency), prefer `iii-queue-processing`.
- For cron scheduling details (expressions, timezones), prefer `iii-cron-scheduling`.
- For OpenTelemetry SDK integration (spans, metrics, traces), prefer `iii-observability`.
- For real-time stream patterns, prefer `iii-realtime-streams`.
- Stay with `iii-engine-config` when the primary problem is configuring or deploying the engine itself.

## When to Use

- Use this skill when the task is primarily about `iii-engine-config` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
