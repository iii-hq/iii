---
name: engine-config
description: >-
  Configures the iii engine via iii-config.yaml and deploys it with Docker.
  Use when setting up modules, adapters, queue configs, ports, env vars,
  production deployments, or Docker Compose orchestration.
---

# Engine Config & Deployment

Comparable to: Infrastructure as code, Docker Compose configs

## Key Concepts

- The **iii-config.yaml** file defines the engine port, modules, and adapters
- **Environment variables** use `${VAR:default}` syntax (default optional)
- **Modules** are the building blocks: API, streams, state, queue, pubsub, cron, otel, etc.
- **Adapters** swap storage backends per module: in_memory, file_based, Redis, RabbitMQ
- **Queue configs** control retry, concurrency, and ordering per queue name
- The engine listens on port **49134** (WebSocket) for SDK/worker connections

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `iii-config.yaml` | Engine configuration file |
| `modules::api::RestApiModule` | HTTP API server (port 3111) |
| `modules::stream::StreamModule` | WebSocket streams (port 3112) |
| `modules::state::StateModule` | Persistent KV state storage |
| `modules::queue::QueueModule` | Background job processing with retries |
| `modules::pubsub::PubSubModule` | In-process event fanout |
| `modules::cron::CronModule` | Time-based scheduling |
| `modules::observability::OtelModule` | OpenTelemetry traces, metrics, logs |
| `modules::http_functions::HttpFunctionsModule` | Outbound HTTP call security |
| `modules::shell::ExecModule` | Spawn external processes |
| `modules::bridge_client::BridgeClientModule` | Distributed cross-engine invocation |
| `modules::telemetry::TelemetryModule` | Anonymous product analytics |

## Common Patterns

- `iii --config ./iii-config.yaml` -- start the engine with a config file
- `curl -fsSL https://install.iii.dev/iii/main/install.sh | sh` -- install the engine
- `docker pull iiidev/iii:latest` -- pull the Docker image
- Dev storage: `store_method: file_based` with `file_path: ./data/...`
- Prod storage: Redis adapters with `redis_url: ${REDIS_URL}`
- Prod queues: RabbitMQ adapter with `amqp_url: ${AMQP_URL}`, `queue_mode: quorum`
- Queue config: `queue_configs: { default: { max_retries: 5, concurrency: 5, type: standard } }`
- Env var with fallback: `port: ${III_PORT:49134}`
- Health check: `curl http://localhost:3111/health`
- Ports: 3111 (API), 3112 (streams), 49134 (engine WS), 9464 (Prometheus)

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full config reference with all module options, adapter configs, and Docker deployment examples.

## Pattern Boundaries

- For HTTP endpoint handler logic, see `http-endpoints`.
- For queue processing patterns, see `queue-processing`.
- For cron scheduling details, see `cron-scheduling`.
- For OpenTelemetry SDK integration, see `observability`.
- For real-time stream patterns, see `realtime-streams`.
- Stay with `engine-config` when the goal is configuring or deploying the engine itself.
