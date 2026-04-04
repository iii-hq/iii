---
title: Configuration (config.yaml)
description: Configure your Motia project within the iii config.yaml file.
---

Motia projects run on [iii](https://iii.dev), which uses a `config.yaml` file for its modular runtime configuration. When you scaffold a project with `npx motia@latest create`, this file is generated for you with sensible defaults.

<Callout type="info">
The `config.yaml` is an iii configuration file. Most modules (API, queue, state, streams, cron, observability) are iii infrastructure concerns. Visit [iii.dev/docs](https://iii.dev/docs) for the full module reference.
</Callout>

## Project Setup

Your `package.json` must have `"type": "module"` and use `iii` as the dev command:

```json title="package.json"
{
  "name": "my-app",
  "type": "module",
  "scripts": {
    "dev": "iii",
    "build": "motia build"
  },
  "dependencies": {
    "motia": "next"
  }
}
```

Running `npm run dev` starts the iii runtime, which reads `config.yaml` and boots all configured modules -- including the Motia application via the Shell Exec module.

---

## Motia-Specific Configuration

The portion of `config.yaml` specific to Motia is the **Shell Exec module**, which tells iii how to build and run your Motia application:

```yaml title="config.yaml (Motia portion)"
  - class: modules::shell::ExecModule
    config:
      watch:
        - steps/**/*.ts
      exec:
        - npx motia dev
        - node dist/index-dev.js
```

- `watch` -- glob patterns for files to watch. When these change, the exec commands are re-run.
- `exec` -- commands to execute in order. `npx motia dev` builds your Steps, and `node dist/index-dev.js` runs the bundled output.

---

## Full Development Config

A complete `config.yaml` for local development. The iii infrastructure modules use file-based storage by default:

```yaml title="config.yaml"
modules:
  - class: modules::stream::StreamModule
    config:
      port: ${STREAM_PORT:3112}
      host: 127.0.0.1
      adapter:
        class: modules::stream::adapters::KvStore
        config:
          store_method: file_based
          file_path: ./data/stream_store

  - class: modules::state::StateModule
    config:
      adapter:
        class: modules::state::adapters::KvStore
        config:
          store_method: file_based
          file_path: ./data/state_store.db

  - class: modules::api::RestApiModule
    config:
      port: 3111
      host: 127.0.0.1
      default_timeout: 30000
      concurrency_request_limit: 1024
      cors:
        allowed_origins:
          - http://localhost:3000
          - http://localhost:5173
          - http://localhost:3113
        allowed_methods:
          - GET
          - POST
          - PUT
          - DELETE
          - OPTIONS

  - class: modules::queue::QueueModule
    config:
      adapter:
        class: modules::queue::BuiltinQueueAdapter
        config:
          store_method: file_based
          file_path: ./data/queue_store

  - class: modules::pubsub::PubSubModule
    config:
      adapter:
        class: modules::pubsub::LocalAdapter

  - class: modules::cron::CronModule
    config:
      adapter:
        class: modules::cron::KvCronAdapter

  - class: modules::observability::OtelModule
    config:
      enabled: ${OTEL_ENABLED:true}
      service_name: ${OTEL_SERVICE_NAME:iii-engine}
      exporter: ${OTEL_EXPORTER_TYPE:memory}

  - class: modules::shell::ExecModule
    config:
      watch:
        - steps/**/*.ts
      exec:
        - npx motia dev
        - node dist/index-dev.js
```

<Callout type="info">
All config values support environment variable interpolation: `${VARIABLE_NAME:default_value}`. If the variable is not set, the default after `:` is used.
</Callout>

For production configuration with Redis adapters and OTLP exporters, see the [Deployment Guide](/docs/deployment-guide) and [iii.dev/docs](https://iii.dev/docs).


---

## What's Next?

<Cards>
  <Card href="/docs/development-guide/streams" title="Streams">
    Build real-time features
  </Card>

  <Card href="/docs/development-guide/state-management" title="State Management">
    Persistent storage across Steps
  </Card>
</Cards>
