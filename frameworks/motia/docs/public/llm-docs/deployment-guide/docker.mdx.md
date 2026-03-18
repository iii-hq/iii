---
title: Docker
description: Deploy Motia applications using Docker containers
---

Docker is the foundation for all Motia production deployments. This guide covers containerizing your Motia app with the iii engine for local testing and production use.

---

## Build Your Project

Before creating a Docker image, build your Motia project:

```bash
motia build
```

This produces optimized production files:

```
dist/
├── index-production.js
└── index-production.js.map
```

---

## Dockerfile

Create a `Dockerfile` in your project root. This uses a multi-stage build to install the iii engine binary and run your Motia app with Bun:

```dockerfile title="Dockerfile"
FROM debian:bookworm-slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://install.iii.dev/iii/main/install.sh | sh

FROM oven/bun:1.1-slim

WORKDIR /app

COPY --from=builder /root/.local/bin/iii /usr/local/bin/iii

COPY package.json .
COPY dist/index-production.js dist/
COPY dist/index-production.js.map dist/
COPY config-production.yaml config.yaml

EXPOSE 3111
EXPOSE 3112

CMD ["iii", "--config", "config.yaml"]
```

### .dockerignore

Create a `.dockerignore` to keep the image small:

```text title=".dockerignore"
node_modules
.git
.env
data/
src/
*.md
```

---

## Production config.yaml

Create a `config-production.yaml` that uses Redis adapters for production:

```yaml title="config-production.yaml"
modules:
  - class: modules::stream::StreamModule
    config:
      port: ${STREAM_PORT:3112}
      host: 0.0.0.0
      adapter:
        class: modules::stream::adapters::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379}

  - class: modules::state::StateModule
    config:
      adapter:
        class: modules::state::adapters::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379}

  - class: modules::api::RestApiModule
    config:
      port: ${PORT:3111}
      host: 0.0.0.0
      default_timeout: 30000
      concurrency_request_limit: 1024

  - class: modules::queue::QueueModule
    config:
      adapter:
        class: modules::queue::BuiltinQueueAdapter

  - class: modules::observability::OtelModule
    config:
      enabled: ${OTEL_ENABLED:true}
      service_name: ${OTEL_SERVICE_NAME:my-app}
      exporter: ${OTEL_EXPORTER_TYPE:memory}

  - class: modules::pubsub::PubSubModule
    config:
      adapter:
        class: modules::pubsub::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379}

  - class: modules::cron::CronModule
    config:
      adapter:
        class: modules::cron::KvCronAdapter

  - class: modules::shell::ExecModule
    config:
      exec:
        - bun run --enable-source-maps dist/index-production.js
```

---

## Docker Compose

For local development with Redis:

```yaml title="docker-compose.yml"
services:
  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"

  app:
    build: .
    ports:
      - "3111:3111"
      - "3112:3112"
    environment:
      - REDIS_URL=redis://redis:6379
    depends_on:
      - redis
```

Run with:

```bash
docker compose up --build
```

---

## Python Steps

Make sure you have the pyproject.toml at your project and the steps folder

```toml title="pyproject"
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "python-test"
version = "0.1.0"
description = "Minimal Motia Python test project"
requires-python = ">=3.10"
dependencies = [
  "motia[otel]",
  "iii-sdk==0.2.0",
]

[tool.hatch.build.targets.wheel]
packages = ["steps"]
```

Include this dockerfile

```dockerfile title="Dockerfile (with Python)"
# syntax=docker/dockerfile:1

FROM debian:bookworm-slim AS iii-builder

RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://install.iii.dev/iii/main/install.sh | sh

FROM python:3.11-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=iii-builder /root/.local/bin/iii /usr/local/bin/iii

COPY pyproject.toml ./
RUN pip install --no-cache-dir uv && \
    uv pip install --system -r pyproject.toml

COPY steps ./steps
COPY config.yaml ./config.yaml

ENV III_URL=ws://127.0.0.1:49134

EXPOSE 3111 3112

CMD ["iii", "-c", "config.yaml"]
```

Add a second ExecModule entry in your `config-production.yaml` for the Python runtime:

```yaml
  - class: modules::shell::ExecModule
    config:
      watch:
        - steps/**/*_step.py
      exec:
        - uv run motia run --dir steps
```

---

## Ports

| Port | Service |
|------|---------|
| `3111` | REST API (HTTP endpoints) |
| `3112` | Stream API (WebSocket) |

---

## What's Next?

<Cards>
  <Card href="/docs/deployment-guide/railway" title="Deploy to Railway">
    One-click deployment with managed Redis
  </Card>
  <Card href="/docs/deployment-guide/fly" title="Deploy to Fly.io">
    Global edge deployment with Upstash Redis
  </Card>
  <Card href="/docs/deployment-guide/self-hosted" title="Self-Hosted">
    Full control with your own infrastructure
  </Card>
</Cards>
