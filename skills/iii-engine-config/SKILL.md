---
name: iii-engine-config
description: >-
  Configures the iii engine via iii-config.yaml — workers, adapters, queue
  configs, ports, and environment variables. Use when deploying, tuning, or
  customizing the engine.
---

# Engine Config

Comparable to: infrastructure as code, worker manifests, runtime policy config

## Key Concepts

Use the concepts below when they fit the task. Not every deployment needs all workers or adapters.

- **config.yaml** / **iii-config.yaml** defines engine workers, modules, adapters, ports, observability, RBAC, and worker manager listeners
- **Environment variables** use `${VAR:default}` syntax (default is optional)
- **Workers** are the building blocks — each enables a capability (API, state, queue, cron, etc.)
- **Managed workers** are registry, binary, OCI image, or local workers controlled by the worker manager and `iii worker` CLI
- **Adapters** swap storage and messaging backends: file/KV, Redis, RabbitMQ, local/in-memory where supported
- **Queue configs** control retry count, concurrency, ordering, and backoff per named queue
- The engine's private worker WebSocket commonly listens on port **49134**
- The console commonly runs on **3113**, HTTP on **3111**, stream WebSocket on **3112**, and Prometheus on **9464** when enabled

## Architecture

The engine loads YAML config at startup, expands environment variables, initializes modules and built-in daemons, opens configured ports, starts the worker manager, then installs or starts managed workers. SDK workers connect over WebSocket; registry-managed binary and OCI workers can be reproduced from `iii.lock`.

## Runtime Workers and Config Surface

| Worker / Config                  | Purpose                                |
| -------------------------------- | -------------------------------------- |
| `iii-http`                       | HTTP API server (port 3111)            |
| `iii-stream`                     | WebSocket streams (port 3112)          |
| `iii-state`                      | Persistent key-value state storage     |
| `iii-queue`                      | Background job processing with retries |
| `iii-pubsub`                     | In-process event fanout                |
| `iii-cron`                       | Time-based scheduling                  |
| `iii-sandbox`                    | MicroVM command/filesystem isolation   |
| `shell`                          | Controlled host/sandbox command and file tools |
| `iii-directory`                  | Engine/registry/skills discovery       |
| `iii-observability`              | OpenTelemetry traces, metrics, logs    |
| `iii-http-functions`             | Outbound HTTP call security            |
| `iii-exec`                       | Spawn external processes               |
| `iii-bridge`                     | Distributed cross-engine invocation    |
| `iii-telemetry`                  | Anonymous product analytics            |
| `iii-worker-manager`             | Worker connection lifecycle and RBAC listeners |
| `iii-worker-ops`                 | Worker lifecycle operations             |
| `iii-engine-functions`           | Built-in engine functions              |
| `iii.lock`                       | Reproducible managed-worker lockfile   |
| `iii worker sync --frozen`       | Verify lockfile without mutation       |

## Code Example

```yaml
workers:
  - name: iii-http
    config:
      host: 127.0.0.1
      port: ${III_HTTP_PORT:3111}

  - name: iii-queue
    config:
      queue_configs:
        payments:
          max_retries: 5
          concurrency: 2
          type: fifo
          message_group_field: orderId
      adapter:
        name: builtin
        config:
          store_method: file_based
          file_path: ./data/queue

  - name: iii-state
    config:
      adapter:
        name: kv
        config:
          store_method: file_based
          file_path: ./data/state

  - name: iii-worker-manager
    config:
      listeners:
        - host: 127.0.0.1
          port: 49134
          private: true
        - host: 0.0.0.0
          port: 49135
          rbac:
            auth_function_id: auth::browser-session
```

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `iii --config ./config.yaml` — start the engine with a config file
- `docker pull iiidev/iii:latest` — pull the Docker image
- Dev storage: `store_method: file_based` with `file_path: ./data/...`
- Prod storage: Redis adapters with `redis_url: ${REDIS_URL}`
- Prod queues: RabbitMQ adapter with `amqp_url: ${AMQP_URL}` and `queue_mode: quorum`
- Queue config: `queue_configs` with `max_retries`, `concurrency`, `type`, `backoff_ms` per queue name
- Env var with fallback: `port: ${III_PORT:49134}`
- Health check: `curl http://127.0.0.1:3111/health`
- Ports: 3111 (API), 3112 (streams), 49134 (engine WS), 9464 (Prometheus)
- RBAC listener: configure `iii-worker-manager` with listener `host`, `port`, `middleware_function_id`, and `rbac`
- HTTP security policy: configure exposed functions, auth function, registration hooks, and forbidden functions on public worker-manager listeners
- Observability: configure OTLP exporter, service name, sampling, metrics, and logs on the observability worker

### Worker Config Format

Workers use `name:` and optional `config:`:

```yaml
workers:
  - name: iii-http
    config:
      port: 3111
      host: 127.0.0.1

  - name: iii-state
    config:
      adapter:
        name: kv
        config:
          store_method: file_based
          file_path: ./data/state_store.db

  - name: iii-queue
    config:
      adapter:
        name: builtin
        config:
          store_method: file_based
          file_path: ./data/queue_store

  - name: iii-stream
    config:
      port: 3112
      host: 127.0.0.1
      adapter:
        name: kv
        config:
          store_method: file_based
          file_path: ./data/stream_store

  - name: iii-cron
    config:
      adapter:
        name: kv

  - name: iii-pubsub
    config:
      adapter:
        name: local

  - name: iii-observability
    config:
      enabled: true
      service_name: my-service
      exporter: memory
      sampling_ratio: 1.0
      metrics_enabled: true
      logs_enabled: true

  - name: iii-sandbox
    config:
      auto_install: true
      image_allowlist:
        - python
        - node
```

### Managed Workers and Lockfiles

- Registry workers are installed with `iii worker add NAME[@VERSION]`.
- Direct OCI workers use image references such as `ghcr.io/org/worker:tag`.
- Local workers point at local binary or development paths when supported by the worker config.
- `iii.lock` records resolved binary artifacts or OCI image digests for reproducible installs.
- Commit `iii.lock` with config. Use `iii worker verify` in CI and `iii worker sync` after cloning.
- Use `iii worker` CLI commands for managed-worker lifecycle, lockfile, and verification workflows.

### RBAC and Security

- Public worker access should go through an RBAC-enabled `iii-worker-manager` listener.
- `auth_function_id` returns allowed and forbidden functions, trigger type permissions, registration permission, registration prefix, and context.
- `forbidden_functions` override exposure filters.
- Discovery is filtered: denied functions should look forbidden, not available.
- Keep RBAC policy examples close to the worker-manager configuration they protect.

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Start with file_based adapters for development, switch to Redis/RabbitMQ for production
- Define queue configs per workload: high-concurrency for parallel jobs, FIFO for ordered processing
- Use environment variables with defaults for all deployment-sensitive values (URLs, ports, credentials)
- Enable only the workers you need — unused workers can be omitted from the config
- Use `iii worker add` to add registry-managed workers, then commit both config and `iii.lock`
- Set `max_retries` and `backoff_ms` based on your failure tolerance and SLA requirements
- Configure the observability worker with your collector endpoint and sampling ratio
- Use `host: 127.0.0.1` instead of `host: localhost` to avoid IPv4/IPv6 mismatches on macOS
- Keep private worker ports bound to localhost unless a listener has explicit RBAC/security policy

## Pattern Boundaries

- For function registration and trigger binding, prefer `iii-functions-and-triggers`.
- For invocation modes, retries, and durable enqueue decisions, prefer `iii-trigger-actions`.
- For built-in trigger config and payload shapes, prefer `iii-trigger-schemas`.
- For SDK instrumentation APIs, prefer the relevant SDK skill.
- For worker-backed HTTP, queue, cron, pubsub, state, stream, observability, lifecycle, lockfile, and RBAC behavior, use the matching worker docs under `engine/src/workers/**/skills`.
- Stay with `iii-engine-config` when the primary problem is configuring or deploying the engine itself.

## When to Use

- Use this skill when the task is primarily about `iii-engine-config` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
