# Engine Config & Deployment -- API Reference

Source: https://iii.dev/docs/how-to/configure-engine, https://iii.dev/docs/advanced/deployment

## Installation

```bash
# Shell script
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh

# Or via Cargo
cargo install iii-engine
```

## CLI Usage

```bash
iii                              # starts with ./config.yaml
iii --config /path/to/config.yaml  # custom config
iii --use-default-config           # built-in defaults
iii --version                      # display version
```

## Environment Variable Syntax

```yaml
port: ${III_PORT:49134}      # with default value
redis_url: ${REDIS_URL}       # required, no default
```

## Top-Level Config

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `port` | integer | 49134 | WebSocket port for SDK/worker connections |
| `modules` | array | required | List of enabled modules |

---

## Modules

### RestApiModule

**Class:** `modules::api::RestApiModule`

```yaml
- class: modules::api::RestApiModule
  config:
    port: 3111
    host: 0.0.0.0
    default_timeout: 30000
    concurrency_request_limit: 1024
    cors:
      allowed_origins: ["*"]
      allowed_methods: ["GET", "POST", "PUT", "DELETE", "PATCH"]
```

| Config | Type | Default | Notes |
|--------|------|---------|-------|
| `port` | int | 3111 | HTTP port |
| `host` | string | 0.0.0.0 | Bind address |
| `default_timeout` | int | 30000 | Request timeout ms |
| `concurrency_request_limit` | int | 1024 | Max concurrent requests |

### StreamModule

**Class:** `modules::stream::StreamModule`

```yaml
- class: modules::stream::StreamModule
  config:
    port: 3112
    host: 0.0.0.0
    auth_function: null
    adapter:
      class: modules::stream::adapters::KvStore
      config:
        store_method: file_based
        file_path: ./data/stream_store
        save_interval_ms: 5000
```

**Adapters:**

- `modules::stream::adapters::KvStore` -- `store_method` (file_based | in_memory), `file_path`, `save_interval_ms`
- `modules::stream::adapters::RedisAdapter` -- `redis_url`
- `modules::stream::adapters::Bridge` -- `bridge_url`

### StateModule

**Class:** `modules::state::StateModule`

```yaml
- class: modules::state::StateModule
  config:
    adapter:
      class: modules::state::adapters::KvStore
      config:
        store_method: file_based
        file_path: ./data/state_store
        save_interval_ms: 5000
```

**Adapters:**

- `modules::state::adapters::KvStore` -- `store_method`, `file_path`, `save_interval_ms`
- `modules::state::adapters::RedisAdapter` -- `redis_url`
- `modules::state::adapters::Bridge` -- `bridge_url`

### QueueModule

**Class:** `modules::queue::QueueModule`

```yaml
- class: modules::queue::QueueModule
  config:
    queue_configs:
      default:
        max_retries: 5
        concurrency: 5
        type: standard
    adapter:
      class: modules::queue::BuiltinQueueAdapter
      config:
        store_method: file_based
        file_path: ./data/queue_store
        save_interval_ms: 5000
```

**queue_configs fields:**

| Field | Purpose |
|-------|---------|
| `max_retries` | Max delivery attempts |
| `concurrency` | Parallel workers per queue |
| `type` | `standard` or `fifo` |

**Adapters:**

- `modules::queue::BuiltinQueueAdapter` -- `max_attempts` (3), `backoff_ms` (1000), `concurrency` (10), `poll_interval_ms` (100), `mode` (concurrent | fifo), `store_method`, `file_path`, `save_interval_ms`
- `modules::queue::RedisAdapter` -- `redis_url`
- `modules::queue::RabbitMQAdapter` -- `amqp_url`, `max_attempts` (3), `prefetch_count` (10), `queue_mode` (standard | quorum)
- `modules::queue::adapters::Bridge` -- `bridge_url`

### PubSubModule

**Class:** `modules::pubsub::PubSubModule`

```yaml
- class: modules::pubsub::PubSubModule
  config:
    adapter:
      class: modules::pubsub::LocalAdapter
```

**Adapters:**

- `modules::pubsub::LocalAdapter` -- no config needed
- `modules::pubsub::RedisAdapter` -- `redis_url`

### CronModule

**Class:** `modules::cron::CronModule`

```yaml
- class: modules::cron::CronModule
  config:
    adapter:
      class: modules::cron::KvCronAdapter
      config:
        store_method: file_based
        file_path: ./data/cron_locks
        save_interval_ms: 5000
```

**Adapters:**

- `modules::cron::KvCronAdapter` -- `lock_ttl_ms` (30000), `lock_index`, `store_method`, `file_path`, `save_interval_ms`
- `modules::cron::RedisCronAdapter` -- `redis_url` (true distributed locking)

### OtelModule

**Class:** `modules::observability::OtelModule`

```yaml
- class: modules::observability::OtelModule
  config:
    enabled: true
    exporter: otlp
    endpoint: ${OTEL_ENDPOINT:http://localhost:4317}
    service_name: iii
    service_version: 1.0.0
    service_namespace: production
    sampling_ratio: 1.0
    level: warn
    format: json
    metrics_enabled: true
    metrics_exporter: memory
    metrics_retention_seconds: 3600
    metrics_max_count: 10000
    logs_enabled: true
    logs_exporter: both
    logs_max_count: 1000
    logs_retention_seconds: 3600
    logs_batch_size: 100
    logs_flush_interval_ms: 5000
    logs_sampling_ratio: 1.0
    logs_console_output: true
    memory_max_spans: 10000
    sampling:
      default: 0.5
      parent_based: true
      rules:
        - operation: "api.*"
          rate: 1.0
      rate_limit:
        max_traces_per_second: 100
    alerts:
      - name: "High Error Rate"
        metric: "iii.invocations.errors"
        threshold: 10
        operator: ">"
        window_seconds: 60
        cooldown_seconds: 300
        action:
          type: webhook
          url: "https://alerts.example.com"
```

| Config | Type | Default | Notes |
|--------|------|---------|-------|
| `enabled` | bool | false | Master switch |
| `exporter` | string | both | memory, otlp, or both |
| `endpoint` | string | localhost:4317 | OTLP collector URL |
| `sampling_ratio` | float | 1.0 | 0.0--1.0 |
| `level` | string | info | trace, debug, info, warn, error |
| `format` | string | json | default or json |
| `metrics_enabled` | bool | true | Enable metrics collection |
| `logs_enabled` | bool | true | Enable log forwarding |
| `memory_max_spans` | int | 10000 | In-memory span buffer |

### HttpFunctionsModule

**Class:** `modules::http_functions::HttpFunctionsModule`

```yaml
- class: modules::http_functions::HttpFunctionsModule
  config:
    security:
      url_allowlist: ["https://api.example.com/*"]
      block_private_ips: true
      require_https: true
```

| Config | Type | Default | Notes |
|--------|------|---------|-------|
| `url_allowlist` | string[] | -- | Allowed URL patterns |
| `block_private_ips` | bool | true | Prevent SSRF |
| `require_https` | bool | true | Enforce HTTPS |

### ExecModule

**Class:** `modules::shell::ExecModule`

```yaml
- class: modules::shell::ExecModule
  config:
    watch:
      - ./steps
    exec:
      - motia-py
```

| Config | Type | Purpose |
|--------|------|---------|
| `watch` | string[] | Directories to monitor |
| `exec` | string[] | Commands to execute |

### BridgeClientModule

**Class:** `modules::bridge_client::BridgeClientModule`

```yaml
- class: modules::bridge_client::BridgeClientModule
  config:
    url: ws://remote-engine:49134
    service_id: client-1
    service_name: Remote Service
    expose:
      - local_function: "my_func"
        remote_function: "exposed_func"
    forward:
      - local_function: "caller"
        remote_function: "target_func"
        timeout_ms: 5000
```

| Config | Type | Required | Purpose |
|--------|------|----------|---------|
| `url` | string | yes | Remote engine WebSocket URL |
| `service_id` | string | yes | Unique identifier |
| `service_name` | string | no | Human-readable name |
| `expose[].local_function` | string | yes | Function to expose remotely |
| `expose[].remote_function` | string | no | Remote name (defaults to local) |
| `forward[].local_function` | string | yes | Local caller function |
| `forward[].remote_function` | string | yes | Remote target function |
| `forward[].timeout_ms` | int | 5000 | Response timeout |

### TelemetryModule

**Class:** `modules::telemetry::TelemetryModule`

```yaml
- class: modules::telemetry::TelemetryModule
  config:
    enabled: true
    api_key: ""
    sdk_api_key: ""
    heartbeat_interval_secs: 21600
```

---

## Complete Development Config

```yaml
port: 49134

modules:
  - class: modules::api::RestApiModule
    config:
      port: 3111
      host: 0.0.0.0
      default_timeout: 30000

  - class: modules::state::StateModule
    config:
      adapter:
        class: modules::state::adapters::KvStore
        config:
          store_method: file_based
          file_path: ./data/state_store

  - class: modules::queue::QueueModule
    config:
      queue_configs:
        default:
          max_retries: 5
          concurrency: 5
          type: standard
      adapter:
        class: modules::queue::BuiltinQueueAdapter
        config:
          store_method: file_based
          file_path: ./data/queue_store

  - class: modules::stream::StreamModule
    config:
      port: 3112
      adapter:
        class: modules::stream::adapters::KvStore
        config:
          store_method: file_based
          file_path: ./data/stream_store

  - class: modules::pubsub::PubSubModule
    config:
      adapter:
        class: modules::pubsub::LocalAdapter

  - class: modules::cron::CronModule
    config:
      adapter:
        class: modules::cron::KvCronAdapter
        config:
          store_method: file_based
          file_path: ./data/cron_locks
```

## Complete Production Config

```yaml
port: ${III_PORT:49134}

modules:
  - class: modules::api::RestApiModule
    config:
      port: ${HTTP_PORT:3111}

  - class: modules::state::StateModule
    config:
      adapter:
        class: modules::state::adapters::RedisAdapter
        config:
          redis_url: ${REDIS_URL}

  - class: modules::queue::QueueModule
    config:
      queue_configs:
        default:
          max_retries: 5
          concurrency: 5
      adapter:
        class: modules::queue::RabbitMQAdapter
        config:
          amqp_url: ${AMQP_URL}
          max_attempts: 3
          prefetch_count: 10
          queue_mode: quorum

  - class: modules::stream::StreamModule
    config:
      port: 3112
      adapter:
        class: modules::stream::adapters::RedisAdapter
        config:
          redis_url: ${REDIS_URL}

  - class: modules::pubsub::PubSubModule
    config:
      adapter:
        class: modules::pubsub::RedisAdapter
        config:
          redis_url: ${REDIS_URL}

  - class: modules::cron::CronModule
    config:
      adapter:
        class: modules::cron::RedisCronAdapter
        config:
          redis_url: ${REDIS_URL}

  - class: modules::observability::OtelModule
    config:
      enabled: true
      exporter: otlp
      endpoint: ${OTEL_ENDPOINT:http://localhost:4317}
      service_name: iii
      sampling_ratio: 0.1

  - class: modules::http_functions::HttpFunctionsModule
    config:
      security:
        url_allowlist: ["https://*.example.com/*"]
        block_private_ips: true
        require_https: true

  - class: modules::telemetry::TelemetryModule
    config:
      enabled: true
```

---

## Docker Deployment

### Docker Image

```bash
docker pull iiidev/iii:latest
```

### Basic Run

```bash
docker run -p 3111:3111 -p 49134:49134 \
  -v ./config.yaml:/app/config.yaml:ro \
  iiidev/iii:latest
```

### Production Hardened Run

```bash
docker run --read-only --tmpfs /tmp \
  --cap-drop=ALL --cap-add=NET_BIND_SERVICE \
  --security-opt=no-new-privileges:true \
  -v ./config.yaml:/app/config.yaml:ro \
  iiidev/iii:latest
```

### Docker Compose (Production)

```yaml
version: "3.8"

services:
  iii:
    image: iiidev/iii:latest
    ports:
      - "3111:3111"
      - "3112:3112"
      - "49134:49134"
      - "9464:9464"
    volumes:
      - ./config.yaml:/app/config.yaml:ro
    environment:
      - REDIS_URL=redis://redis:6379
      - AMQP_URL=amqp://rabbitmq:5672
      - OTEL_ENDPOINT=http://otel-collector:4317
    depends_on:
      - redis
      - rabbitmq

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"

  rabbitmq:
    image: rabbitmq:3-management
    ports:
      - "5672:5672"
      - "15672:15672"
```

### Worker Dockerfile

```dockerfile
FROM node:20-slim
WORKDIR /app
COPY package*.json ./
RUN npm ci --production
COPY . .
CMD ["node", "worker.js"]
```

### Health Checks

```bash
# HTTP API
curl http://localhost:3111/health

# WebSocket engine
nc -zv 127.0.0.1 49134

# Prometheus metrics
curl http://localhost:9464/metrics
```

## Ports Summary

| Port | Protocol | Purpose |
|------|----------|---------|
| 3111 | HTTP | REST API endpoints |
| 3112 | WebSocket | Real-time streams |
| 49134 | WebSocket | Engine worker connections |
| 9464 | HTTP | Prometheus metrics |

## Key Notes

- Do not use `in_memory` storage in production -- data is lost on restart.
- Use `file_based` for development with persistence.
- Use Redis, RabbitMQ, or Bridge adapters for production distributed systems.
- For multi-instance deployments, all adapters must use Redis/RabbitMQ to share state.
- Cron uses Redis-based distributed locks to prevent duplicate execution across instances.
