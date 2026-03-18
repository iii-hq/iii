---
title: Self-Hosted Deployment
description: Learn how to deploy your Motia project to production using motia-docker
---

## Prerequisites

Before you build your Docker image, first make sure you run `motia build` to build your project.

```bash
motia build
```

This will build your project and create a `dist` directory with your production-ready code.
It should be just two files: `index-production.js` and `index-production.js.map`.

```
dist/
├── index-production.js
└── index-production.js.map
```

## iii Production Config

Make sure you have a `config-production.yaml` file in your project.

```yaml title="config-production.yaml"
modules:
  - class: modules::stream::StreamModule
    config:
      port: ${STREAM_PORT:3112}
      host: 0.0.0.0
      adapter:
        class: modules::stream::adapters::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379} # make sure the redis url is correct

  - class: modules::state::StateModule
    config:
      adapter:
        class: modules::state::adapters::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379} # make sure the redis url is correct

  # Add CORS
  - class: modules::api::RestApiModule
    config:
      port: 3111 # make sure the port meets your load balancer's requirements
      host: 0.0.0.0 # make sure it has access to the internet
      default_timeout: 30000
      concurrency_request_limit: 1024
      cors:
        allowed_origins:
          - https://app.example.com
        allowed_methods:
          - GET
          - POST
          - PUT
          - DELETE
          - OPTIONS

  # Optional: Configure an OTEL server to send telemetry to
  - class: modules::observability::OtelModule
    config:
      enabled: ${OTEL_ENABLED:true}
      service_name: ${OTEL_SERVICE_NAME:iii-engine}
      service_version: ${SERVICE_VERSION:0.2.0}
      service_namespace: ${SERVICE_NAMESPACE:production}
      exporter: ${OTEL_EXPORTER_TYPE:memory}
      endpoint: ${OTEL_EXPORTER_OTLP_ENDPOINT:http://localhost:4317}
      sampling_ratio: 1.0
      memory_max_spans: ${OTEL_MEMORY_MAX_SPANS:10000}
      metrics_enabled: true
      metrics_exporter: ${OTEL_METRICS_EXPORTER:memory}
      metrics_retention_seconds: 3600
      metrics_max_count: 10000
      logs_enabled: ${OTEL_LOGS_ENABLED:true}
      logs_exporter: ${OTEL_LOGS_EXPORTER:memory}
      logs_max_count: ${OTEL_LOGS_MAX_COUNT:1000}
      logs_retention_seconds: ${OTEL_LOGS_RETENTION_SECONDS:3600}
      logs_sampling_ratio: ${OTEL_LOGS_SAMPLING_RATIO:1.0}

  - class: modules::queue::QueueModule
    config:
      adapter:
        # We have a binary that has RabbitMQ adapter, but is not published yet
        # This built-in adapter is for 1 single instance
        class: modules::queue::BuiltinQueueAdapter

  - class: modules::pubsub::PubSubModule
    config:
      adapter:
        class: modules::pubsub::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379} # make sure the redis url is correct

  - class: modules::cron::CronModule
    config:
      adapter:
        class: modules::cron::KvCronAdapter

  - class: modules::shell::ExecModule
    config:
      exec:
        - bun run --enable-source-maps dist/index-production.js
```

## Docker Setup

```dockerfile title="Dockerfile"
FROM debian:bookworm-slim AS builder

# Install curl and ca-certificates
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Install iii
RUN curl -fSL https://install.iii.dev/iii/main/install.sh | sh

FROM oven/bun:1.1-slim

WORKDIR /app

# Copy iii binary
COPY --from=builder /root/.local/bin/iii /usr/local/bin/iii

COPY package.json .
COPY dist/index-production.js dist/
COPY dist/index-production.js.map dist/
COPY config-production.yaml config.yaml

EXPOSE 3111
EXPOSE 3112

CMD ["iii", "--config", "config.yaml"]
```

### Python Steps?

Use the Dockerfile setup documented in [Docker guide - Python Steps](/docs/deployment-guide/docker#python-steps).

---

## Deploy to Cloud

Once you have Docker working locally, deploy to any cloud platform:

### Railway

The easiest option. Railway detects your Dockerfile automatically and provides managed Redis.

👉 [Full Railway deployment guide →](/docs/deployment-guide/railway)

### Fly.io

Global edge deployment with Upstash Redis. Great for low-latency worldwide.

👉 [Full Fly.io deployment guide →](/docs/deployment-guide/fly)
