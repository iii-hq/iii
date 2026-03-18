---
title: The iii Engine
description: How iii manages infrastructure for Motia through config.yaml modules
---

Motia is the application framework — you write Steps in TypeScript, Python, or JavaScript. The **iii engine** is the runtime that powers everything underneath. It manages queues, state storage, stream servers, cron scheduling, HTTP routing, observability, and the lifecycle of your application processes.

## How Motia and iii Work Together

```
┌─────────────────────────────────────────────┐
│                 iii Engine                  │
│                                             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │  Queue   │  │  State   │  │  Stream  │   │
│  │  Module  │  │  Module  │  │  Module  │   │
│  └──────────┘  └──────────┘  └──────────┘   │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │  REST    │  │   Cron   │  │  OTel    │   │
│  │  API     │  │  Module  │  │  Module  │   │
│  └──────────┘  └──────────┘  └──────────┘   │
│  ┌──────────┐  ┌──────────┐                 │
│  │  PubSub  │  │   Exec   │  ← manages      │
│  │  Module  │  │  Module  │    SDK process  │
│  └──────────┘  └──────────┘                 │
│                     │                       │
│                     ▼                       │
│            ┌─────────────────┐              │
│            │   Motia SDK     │              │
│            │  (your Steps)   │              │
│            └─────────────────┘              │
└─────────────────────────────────────────────┘
```

The iii engine reads a `config.yaml` file that declares which modules to load and how to configure them. It then starts and manages your Motia application through the **ExecModule**.

The iii development console gives you full visibility into the running engine — modules, functions, triggers, streams, and workers:

![Configuration overview in the iii Console](/console/config-overview.png)

---

## config.yaml

The `config.yaml` file is the single source of truth for all infrastructure configuration. It replaces the old `motia.config.ts` plugin system — no more JavaScript configuration for infrastructure concerns.

```yaml
modules:
  - class: modules::api::RestApiModule
    config:
      port: 3111
      host: 0.0.0.0

  - class: modules::queue::QueueModule
    config:
      adapter:
        class: modules::queue::BuiltinQueueAdapter

  - class: modules::state::StateModule
    config:
      adapter:
        class: modules::state::adapters::KvStore
        config:
          store_method: file_based
          file_path: ./data/state_store.db

  - class: modules::shell::ExecModule
    config:
      watch:
        - steps/**/*.ts
        - motia.config.ts
      exec:
        - npx motia dev
```

---

## Core Modules

### REST API Module

Serves HTTP endpoints defined by your Step triggers. Configures port, host, CORS, timeouts, and concurrency limits.

```yaml
- class: modules::api::RestApiModule
  config:
    port: 3111
    host: 0.0.0.0
    default_timeout: 30000
    concurrency_request_limit: 1024
    cors:
      allowed_origins:
        - http://localhost:3000
      allowed_methods:
        - GET
        - POST
        - PUT
        - DELETE
        - OPTIONS
```

### Queue Module

Manages message queues for async Step-to-Step communication via `enqueue()`. Supports built-in, Redis, and RabbitMQ adapters.

```yaml
- class: modules::queue::QueueModule
  config:
    adapter:
      class: modules::queue::BuiltinQueueAdapter
      # For Redis:
      #   class: modules::queue::RedisAdapter
      #   config: { redis_url: "redis://localhost:6379" }
      # For RabbitMQ:
      #   class: modules::queue::RabbitMQAdapter
      #   config: { amqp_url: "amqp://localhost:5672" }
```

### State Module

Key-value state storage grouped by namespace. Supports file-based, in-memory, and Redis adapters.

```yaml
- class: modules::state::StateModule
  config:
    adapter:
      class: modules::state::adapters::KvStore
      config:
        store_method: file_based
        file_path: ./data/state_store.db
```

### Stream Module

Manages real-time data streams with WebSocket support. Supports KvStore and Redis adapters.

```yaml
- class: modules::stream::StreamModule
  config:
    port: 3112
    host: 0.0.0.0
    adapter:
      class: modules::stream::adapters::KvStore
      config:
        store_method: file_based
        file_path: ./data/stream_store
```

### Cron Module

Schedules and executes cron-based triggers.

```yaml
- class: modules::cron::CronModule
  config:
    adapter:
      class: modules::cron::KvCronAdapter
```

### PubSub Module

Internal publish/subscribe messaging between engine components.

```yaml
- class: modules::pubsub::PubSubModule
  config:
    adapter:
      class: modules::pubsub::LocalAdapter
      # For Redis:
      #   class: modules::pubsub::RedisAdapter
      #   config: { redis_url: "redis://localhost:6379" }
```

### OpenTelemetry Module

Distributed traces, metrics, and structured logs for observability.

```yaml
- class: modules::observability::OtelModule
  config:
    enabled: true
    service_name: my-service
    service_version: 0.1.0
    exporter: memory
    sampling_ratio: 1.0
    metrics_enabled: true
    metrics_exporter: memory
    logs_enabled: true
    logs_exporter: memory
    logs_max_count: 1000
```

### Exec Module

Manages the lifecycle of your Motia SDK process. Watches files for changes and restarts on hot-reload.

```yaml
- class: modules::shell::ExecModule
  config:
    watch:
      - steps/**/*.ts
      - motia.config.ts
    exec:
      - npx motia dev
```

The `exec` array lists the commands to run your SDK process. The `watch` array lists glob patterns — when matching files change, iii restarts the process automatically.

---

## Adapter Swapping

Every module that manages data (queues, state, streams, cron, pubsub) supports multiple **adapters**. This lets you use lightweight local adapters during development and swap to production-grade infrastructure without changing your application code.

| Module | Local Adapter | Production Adapter |
|---|---|---|
| Queue | `BuiltinQueueAdapter` | `RedisAdapter`, `RabbitMQAdapter` |
| State | `KvStore` (file_based) | `RedisAdapter` |
| Stream | `KvStore` (file_based) | `RedisAdapter` |
| Cron | `KvCronAdapter` | `RedisCronAdapter` |
| PubSub | `LocalAdapter` | `RedisAdapter` |

To swap adapters, change the `class` field in `config.yaml` — no application code changes needed.

---

## Environment Variable Interpolation

Use `${VAR:default}` syntax in config.yaml for environment-specific values:

```yaml
- class: modules::api::RestApiModule
  config:
    port: ${API_PORT:3111}
    host: ${API_HOST:0.0.0.0}
```

---

## Multi-Runtime Projects

For projects that use both Node.js and Python, configure separate ExecModule entries:

```yaml
modules:
  - class: modules::shell::ExecModule
    config:
      watch:
        - steps/**/*.ts
      exec:
        - npx motia dev

  - class: modules::shell::ExecModule
    config:
      watch:
        - steps/**/*.py
      exec:
        - uv run motia dev --dir steps
```

Each runtime runs as an independent process managed by iii. Python developers do not need Node.js installed, and vice versa.

---

## Running iii

Start the iii engine with:

```bash
iii -c config.yaml
```

This starts all configured modules and the Motia SDK process. iii handles hot-reloading, process management, and infrastructure lifecycle automatically.

---

## What's Next?

<Cards>
  <Card title="Steps & Triggers" href="/docs/concepts/steps">
    Learn how Steps and triggers define your application logic.
  </Card>
  <Card title="Motia Config Reference" href="/docs/development-guide/motia-config">
    Detailed reference for all config.yaml module options.
  </Card>
</Cards>
