---
title: Deploy to Fly.io
description: Deploy your Motia app to Fly.io with Upstash Redis for global edge deployment
---

Fly.io runs your app on fast micro-VMs close to your users. Combined with Upstash Redis, you get a globally distributed Motia backend.

This guide walks you through deploying a Motia app to Fly.io with production Redis.

<Callout type="info">
**What you'll get:** A containerized Motia app running on Fly.io with Upstash Redis for state, events, streams, and cron locking.
</Callout>

<Callout type="info">
**Example Project:** Follow along with the [Todo App example](https://github.com/MotiaDev/motia-examples/tree/main/examples/foundational/api-patterns/todo-app) - a complete deployment-ready Motia app with Redis configuration.
</Callout>

---

## Prerequisites

Before you start:

- A [Fly.io account](https://fly.io) (free tier works)
- [Fly CLI](https://fly.io/docs/hands-on/install-flyctl/) installed
- Docker running locally (for testing)
- A Motia project ready to deploy

Install the Fly CLI:

```bash
# macOS
brew install flyctl

# Windows
powershell -Command "iwr https://fly.io/install.ps1 -useb | iex"

# Linux
curl -L https://fly.io/install.sh | sh
```

Login:

```bash
flyctl auth login
```

---

## Quick Start

<Steps>
<Step>
#### Build your project and create Dockerfile

Build your Motia project first:

```bash
motia build
```

Then create a `Dockerfile` in your project root:

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

</Step>
<Step>
#### Create fly.toml

```toml title="fly.toml"
app = "my-motia-app"
primary_region = "sjc"

[build]
  dockerfile = "Dockerfile"

[http_service]
  internal_port = 3111
  force_https = true
  auto_stop_machines = false
  auto_start_machines = true
  min_machines_running = 1
  processes = ["app"]

[[vm]]
  memory = "1gb"
  cpu_kind = "shared"
  cpus = 1
```

Replace `my-motia-app` with your app name and `sjc` with your preferred [region](https://fly.io/docs/reference/regions/).

</Step>
<Step>
#### Launch your app

```bash
flyctl launch --no-deploy
```

This creates your app on Fly without deploying yet.

</Step>
<Step>
#### Add Upstash Redis

```bash
flyctl redis create
```

Follow the prompts to create a Redis instance. Choose a region close to your app.

<Callout type="warn">
Upstash Redis on Fly requires a credit card on file, even for free tier usage.
</Callout>

</Step>
<Step>
#### Set environment variables

```bash
flyctl secrets set NODE_ENV=production USE_REDIS=true
```

The Redis URL is automatically attached when you create Redis with `flyctl redis create`.

</Step>
<Step>
#### Deploy

```bash
flyctl deploy
```

Fly builds your Docker image and deploys it globally.

</Step>
<Step>
#### Get your URL

Your app is live at: `https://my-motia-app.fly.dev`

</Step>
</Steps>

---

## Project Setup

### Configure the iii Engine

The iii engine is the entrypoint for your Motia app in production. It reads `config.yaml` and manages all modules. Make sure your `config.yaml` binds to `0.0.0.0` -- Fly needs your app to listen on all interfaces.

Use `${PORT:3111}` in your REST API module config so Fly can inject its port if needed.

### Configure Redis

Create a production `config.yaml` with Redis adapters for all modules. Fly sets `REDIS_URL` when you run `flyctl redis create`:

```yaml title="config-production.yaml"
modules:
  - class: modules::stream::StreamModule
    config:
      port: ${STREAM_PORT:3112}
      host: 0.0.0.0
      auth_function: motia.streams.authenticate
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
        class: modules::queue::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379}

  - class: modules::observability::OtelModule
    config:
      enabled: true
      service_name: ${OTEL_SERVICE_NAME:my-app}
      exporter: otlp

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

<Callout type="info">
The `${REDIS_URL:redis://localhost:6379}` syntax uses environment variable interpolation with a default value. Fly auto-provisions this variable when you attach Upstash Redis.
</Callout>

---

## Fly.io vs Railway

| Feature | Fly.io | Railway |
|---------|--------|---------|
| Global regions | 30+ regions | Limited regions |
| Redis | Upstash (external) | Built-in Redis |
| Pricing | Pay-per-use | Usage-based |
| CLI | `flyctl` | `railway` |
| Best for | Edge deployment | Simple setup |

Choose Fly.io if you need low-latency responses globally. Choose Railway for simpler Redis setup.

---

## Common Commands

```bash
# Check app status
flyctl status

# View logs (streams in real-time)
flyctl logs

# SSH into your app
flyctl ssh console

# Scale machines
flyctl scale count 3

# List secrets
flyctl secrets list

# Destroy app
flyctl apps destroy my-motia-app
```

---

## Troubleshooting

### App Not Listening on Expected Address

**Symptom:** Fly warns "app is not listening on the expected address"

**Fix:** Make sure your `config.yaml` REST API module uses `host: 0.0.0.0` and the port matches `fly.toml`'s `internal_port`:

```yaml
  - class: modules::api::RestApiModule
    config:
      port: ${PORT:3111}
      host: 0.0.0.0
```

### Redis Connection Refused

**Symptom:** Logs show `ECONNREFUSED 127.0.0.1:6379`

**Cause:** App is trying to connect to local Redis instead of Upstash.

**Fix:**
1. Check if `REDIS_URL` is set: `flyctl secrets list`
2. Create Redis if missing: `flyctl redis create`
3. Verify your config parses the URL correctly

### TLS Connection Errors

**Symptom:** Redis connection fails with TLS/SSL errors

**Cause:** Upstash requires TLS (`rediss://`), but your config isn't enabling it.

**Fix:** Make sure your config enables TLS when the protocol is `rediss://`:

```typescript
const useTls = url.protocol === 'rediss:'
```

### Machine Keeps Restarting

**Symptom:** App restarts repeatedly in logs

**Common causes:**
1. Unhandled exceptions during startup
2. Missing environment variables
3. Port binding issues

**Debug:** Check logs for the actual error:

```bash
flyctl logs
```

---

## Scaling Globally

Fly makes global deployment easy. Add machines in different regions:

```bash
# Add a machine in Amsterdam
flyctl machine clone --region ams

# Add a machine in Sydney  
flyctl machine clone --region syd
```

With Redis configured, all machines share state automatically. Users connect to the nearest machine for lowest latency.

---

## What's Next?

<Cards>
  <Card href="/docs/deployment-guide/railway" title="Deploy to Railway">
    Alternative with simpler Redis setup
  </Card>
  
  <Card href="/docs/deployment-guide/self-hosted" title="Self-Hosted">
    Full control with your own infrastructure
  </Card>
</Cards>
