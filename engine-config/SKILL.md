---
name: engine-config
description: >-
  Configures the iii engine via iii-config.yaml — modules, adapters, queue
  configs, ports, and environment variables. Use when deploying, tuning, or
  customizing the engine.
---

# Engine Config

Comparable to: Infrastructure as code, Docker Compose configs

## Key Concepts

Use the concepts below when they fit the task. Not every deployment needs all modules or adapters.

- **iii-config.yaml** defines the engine port, modules, adapters, and queue configs
- **Environment variables** use `${VAR:default}` syntax (default is optional)
- **Modules** are the building blocks — each enables a capability (API, state, queue, cron, etc.)
- **Adapters** swap storage backends per module: in_memory, file_based, Redis, RabbitMQ
- **Queue configs** control retry count, concurrency, ordering, and backoff per named queue
- The engine listens on port **49134** (WebSocket) for SDK/worker connections

## Architecture

The iii-config.yaml file is loaded by the iii engine binary at startup. Modules are initialized in order, adapters connect to their backends, and the engine begins accepting worker connections over WebSocket on port 49134.

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

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Start with file_based adapters for development, switch to Redis/RabbitMQ for production
- Define queue configs per workload: high-concurrency for parallel jobs, FIFO for ordered processing
- Use environment variables with defaults for all deployment-sensitive values (URLs, ports, credentials)
- Enable only the modules you need — unused modules can be omitted from the config
- Set `max_retries` and `backoff_ms` based on your failure tolerance and SLA requirements
- Configure `OtelModule` with your collector endpoint and sampling ratio for observability

## Pattern Boundaries

- For HTTP handler logic (request/response, path params), prefer `http-endpoints`.
- For queue processing patterns (enqueue, FIFO, concurrency), prefer `queue-processing`.
- For cron scheduling details (expressions, timezones), prefer `cron-scheduling`.
- For OpenTelemetry SDK integration (spans, metrics, traces), prefer `observability`.
- For real-time stream patterns, prefer `realtime-streams`.
- Stay with `engine-config` when the primary problem is configuring or deploying the engine itself.

## When to Use

- Use this skill when the task is primarily about `engine-config` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
