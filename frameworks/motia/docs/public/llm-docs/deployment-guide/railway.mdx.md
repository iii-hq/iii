---
title: Deploy to Railway
description: Deploy your Motia app to Railway with Redis for production-grade backends
---

Railway makes deploying Motia apps dead simple. Connect your repo, add Redis, and you're live in minutes.

This guide walks you through deploying a production-ready Motia app with real Redis (not in-memory) on Railway.

<Callout type="info">
**What you'll get:** A fully containerized Motia app running on Railway with external Redis for state, events, streams, and cron locking.
</Callout>

<Callout type="info">
**Example Project:** Follow along with the [Todo App example](https://github.com/MotiaDev/motia-examples/tree/main/examples/foundational/api-patterns/todo-app) - a complete deployment-ready Motia app with Redis configuration.
</Callout>

---

## Prerequisites

Before you start:

- A [Railway account](https://railway.com) (free tier works)
- [Railway CLI](https://docs.railway.com/guides/cli) installed
- Docker running locally (for testing)
- A Motia project ready to deploy

Install the Railway CLI:

```bash
npm install -g @railway/cli
```

---

## Quick Start

<Steps>
<Step>
#### Login to Railway

```bash
railway login
```

This opens your browser for authentication.

</Step>
<Step>
#### Initialize your project

From your Motia project root:

```bash
railway init -n my-motia-app
```

This creates a new Railway project with the specified name.

</Step>
<Step>
#### Add Redis

```bash
railway add -d redis
```

Railway provisions a managed Redis instance automatically.

</Step>
<Step>
#### Create an app service

```bash
railway add -s my-app
```

This creates an empty service for your Motia app.

</Step>
<Step>
#### Link and configure

```bash
# Link to your app service
railway service my-app

# Set environment variables
railway variables --set "NODE_ENV=production"
railway variables --set "USE_REDIS=true"
railway variables --set 'REDIS_URL=${{Redis.REDIS_URL}}'
railway variables --set 'REDIS_PRIVATE_URL=${{Redis.REDIS_PRIVATE_URL}}'
```

</Step>
<Step>
#### Deploy

```bash
railway up
```

Railway builds your Docker image and deploys it.

</Step>
<Step>
#### Get your public URL

```bash
railway domain
```

This assigns a public URL to your app. You're live!

</Step>
</Steps>

---

## Project Setup

### Build Your Project

Before creating the Docker image, build your Motia project:

```bash
motia build
```

This creates optimized production files in `dist/`.

### Dockerfile

Create a `Dockerfile` in your project root. This uses the iii engine as the entrypoint:

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

### Railway Configuration

Create a `railway.json` in your project root:

```json title="railway.json"
{
  "$schema": "https://railway.app/railway.schema.json",
  "build": {
    "builder": "DOCKERFILE",
    "dockerfilePath": "Dockerfile"
  },
  "deploy": {
    "numReplicas": 1,
    "restartPolicyType": "ON_FAILURE",
    "restartPolicyMaxRetries": 10
  }
}
```

<Callout type="info">
**Healthchecks:** Railway's default healthcheck expects a `200` response on `/`. The iii engine serves this by default. Check the configuration if Railway's healthcheck fails.
</Callout>

---

## Configure Redis

Create a production `config.yaml` with Redis adapters for all modules. Railway auto-injects `REDIS_URL` when you link the Redis service to your app:

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
The `${REDIS_URL:redis://localhost:6379}` syntax uses environment variable interpolation with a default value. Railway auto-provisions this variable when you add a Redis database.
</Callout>

---

## Set Environment Variables

Railway auto-provisions Redis variables when you add the Redis database. Link them to your app:

<Steps>
<Step>
#### Link to your app service

```bash
railway service my-app
```

Select your app service (not Redis).

</Step>
<Step>
#### Set the variables

```bash
railway variables --set "NODE_ENV=production"
railway variables --set "USE_REDIS=true"
railway variables --set 'REDIS_URL=${{Redis.REDIS_URL}}'
railway variables --set 'REDIS_PRIVATE_URL=${{Redis.REDIS_PRIVATE_URL}}'
```

The `${{Redis.REDIS_URL}}` syntax tells Railway to inject the actual Redis URL at runtime.

</Step>
<Step>
#### Verify variables

```bash
railway variables
```

You should see your variables listed.

</Step>
</Steps>

<Callout type="warn">
**Internal vs Public URL:** Railway provides both internal (`redis.railway.internal`) and public proxy URLs for Redis. Use `REDIS_PRIVATE_URL` for faster internal connections. If you have connection issues, try the public URL from your Redis service's settings.
</Callout>

---

## Deploy and Test

### Deploy Your App

```bash
railway up
```

Watch the build logs. Once complete, Railway deploys your container.

### Get Your Domain

```bash
railway domain
```

Railway assigns a public URL like `https://your-app-production-xxxx.up.railway.app`.

### Test Your API

```bash
# List items (should be empty initially)
curl https://your-app-production-xxxx.up.railway.app/todos

# Create an item
curl -X POST https://your-app-production-xxxx.up.railway.app/todos \
  -H "Content-Type: application/json" \
  -d '{"title":"Test from Railway","priority":"high"}'

# List items again (should show your new item)
curl https://your-app-production-xxxx.up.railway.app/todos
```

If you get a JSON response with your data, you're running on production Redis!

---

## View Logs

Check what's happening in your deployed app:

```bash
railway logs
```

Add `--tail` to stream logs in real-time:

```bash
railway logs --tail
```

---

## Troubleshooting

### 502 Application Failed to Respond

**Cause:** Usually means the app isn't listening on the right port.

**Fix:** Make sure your `config.yaml` uses `${PORT:3111}` for the REST API module port so Railway can inject its port:

```yaml
  - class: modules::api::RestApiModule
    config:
      port: ${PORT:3111}
      host: 0.0.0.0
```

### Redis Connection Errors

**Cause:** The app can't reach Redis.

**Check:**
1. Is the Redis service running? Check Railway dashboard.
2. Is `REDIS_URL` set correctly? Run `railway variables` to verify.
3. Try the public Redis URL if internal isn't working.

### Healthcheck Failed

**Cause:** Railway expects a 200 response on your healthcheck path.

**Options:**
1. Remove healthcheck settings from `railway.json`
2. The iii runtime serves `/` by default which returns 200
3. Increase the healthcheck timeout

### Still Seeing "Redis Memory Server Started"

**Cause:** The app is falling back to in-memory Redis.

**Check:**
1. Is `NODE_ENV=production` or `USE_REDIS=true` set?
2. Is `REDIS_URL` resolving correctly?
3. Check logs for Redis connection errors.

---

## Scaling

Need more instances? Update your `railway.json`:

```json title="railway.json"
{
  "deploy": {
    "numReplicas": 3
  }
}
```

With Redis configured, all instances share state, events, and streams. Requests get load-balanced automatically.

---

## What's Next?

<Cards>
  <Card href="/docs/deployment-guide/fly" title="Deploy to Fly.io">
    Alternative cloud platform with global edge deployment
  </Card>
  
  <Card href="/docs/deployment-guide/self-hosted" title="Self-Hosted">
    Full control with your own infrastructure
  </Card>
</Cards>
