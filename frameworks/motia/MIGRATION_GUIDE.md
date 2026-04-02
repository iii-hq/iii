# Motia Migration Guide

This guide covers migrating from **Motia v0.17.x** to the **Motia 1.0-RC** framework. It is organized by area of concern so you can migrate incrementally.

---

## Table of Contents

1. [Configuration](#1-configuration)
2. [Module System and Runtime](#2-module-system-and-runtime)
3. [Steps and Triggers -- Unified Config Model](#3-steps-and-triggers----unified-config-model)
4. [HTTP Triggers](#4-http-triggers)
5. [Queue Triggers (formerly Event Steps)](#5-queue-triggers-formerly-event-steps)
6. [Cron Triggers](#6-cron-triggers)
7. [Streams](#7-streams)
8. [State](#8-state)
9. [Middleware](#9-middleware)
10. [New Features](#10-new-features)
11. [Migration Checklist](#11-migration-checklist)
12. [Python Runtime](#12-python-runtime)
13. [Workbench, Plugins, and Console](#13-workbench-plugins-and-console)
14. [OpenAPI Generation](#14-openapi-generation)

---

## 1. Configuration

### iii Engine (New Requirement)

Motia now requires the **iii engine** to run. The iii engine is the Rust-powered runtime that manages all modules (streams, state, API, queues, cron, observability) and orchestrates the SDK process. All adapter and infrastructure configuration is done through iii via a `config.yaml` file -- the SDK itself no longer handles any of this.

Install iii from [https://iii.dev](https://iii.dev) before proceeding with the migration.

### Project Config

The old `motia.config.ts` (using `defineConfig`) is replaced by two files managed by iii:

| Concern | Old | New |
|---|---|---|
| Project config & plugins | `motia.config.ts` (`defineConfig({...})`) | Removed (handled by iii engine via `config.yaml`) |
| Module/adapter config | N/A | `config.yaml` (iii engine config) |
| Auth & hooks | `streamAuth` in `motia.config.ts` | `motia.config.ts` (simplified, exports only auth hooks) |
| Build externals | `.esbuildrc.json` | Removed |
| Workbench UI layout | `motia-workbench.json` | Removed (see [Workbench, Plugins, and Console](#13-workbench-plugins-and-console)) |

**Old -- `motia.config.ts`:**

```typescript
import path from 'node:path'
import { defineConfig, type MotiaPlugin, type MotiaPluginContext, type StreamAuthRequest } from '@motiadev/core'
import bullmqPlugin from '@motiadev/plugin-bullmq/plugin'
import endpointPlugin from '@motiadev/plugin-endpoint/plugin'
import examplePlugin from '@motiadev/plugin-example/plugin'
import logsPlugin from '@motiadev/plugin-logs/plugin'
import observabilityPlugin from '@motiadev/plugin-observability/plugin'
import statesPlugin from '@motiadev/plugin-states/plugin'
import { z } from 'zod'

const streamAuthContextSchema = z.object({
  userId: z.string(),
  permissions: z.enum(['nodejs', 'python']).optional(),
})

const demoTokens: Record<string, z.infer<typeof streamAuthContextSchema>> = {
  'token-nodejs': { userId: 'anderson', permissions: 'nodejs' },
  'token-python': { userId: 'sergio', permissions: 'python' },
}

const extractAuthToken = (request: StreamAuthRequest): string | undefined => {
  const protocol = request.headers['sec-websocket-protocol'] as string | undefined
  if (protocol?.includes('Authorization')) {
    const [, token] = protocol.split(',')
    if (token) return token.trim()
  }
  try {
    const url = new URL(request.url)
    return url.searchParams.get('authToken') ?? undefined
  } catch {
    return undefined
  }
}

export default defineConfig({
  plugins: [
    observabilityPlugin,
    statesPlugin,
    endpointPlugin,
    logsPlugin,
    examplePlugin,
    bullmqPlugin,
  ],
  streamAuth: {
    contextSchema: z.toJSONSchema(streamAuthContextSchema),
    authenticate: async (request: StreamAuthRequest) => {
      const token = extractAuthToken(request)
      if (!token) return null
      const tokenData = demoTokens[token]
      if (!tokenData) throw new Error(`Invalid token: ${token}`)
      return tokenData
    },
  },
})
```

**New -- `config.yaml` (development):**

```yaml
modules:
  # ── Stream Module ──────────────────────────────────────────────────────
  # Manages real-time data streams with WebSocket support.
  # Adapters: KvStore (file_based | in_memory), RedisAdapter
  - class: modules::stream::StreamModule
    config:
      port: ${STREAM_PORT:3112}       # WebSocket server port (default: 3112)
      host: 0.0.0.0                   # Host address to bind (default: 0.0.0.0)
      # auth_function: motia.stream.authenticate  # Reference to auth fn in motia.config.ts
      adapter:
        class: modules::stream::adapters::KvStore
        config:
          store_method: file_based    # "file_based" or "in_memory" (default: in_memory)
          file_path: ./data/stream_store  # Directory for file-based persistence
          # save_interval_ms: 5000    # Disk flush interval in ms (default: 5000)

  # ── State Module ───────────────────────────────────────────────────────
  # Key-value state storage grouped by namespace.
  # Adapters: KvStore (file_based | in_memory), RedisAdapter
  - class: modules::state::StateModule
    config:
      adapter:
        class: modules::state::adapters::KvStore
        config:
          store_method: file_based    # "file_based" or "in_memory" (default: in_memory)
          file_path: ./data/state_store.db  # Directory for file-based persistence
          # save_interval_ms: 5000    # Disk flush interval in ms (default: 5000)

  # ── REST API Module ────────────────────────────────────────────────────
  # Serves HTTP endpoints defined by step triggers.
  - class: modules::api::RestApiModule
    config:
      port: 3111                      # HTTP server port (default: 3111)
      host: 0.0.0.0                   # Host address to bind (default: 0.0.0.0)
      default_timeout: 30000          # Request timeout in ms (default: 30000)
      concurrency_request_limit: 1024 # Max concurrent requests (default: 1024)
      cors:
        allowed_origins:              # Origins allowed to make cross-origin requests
          - http://localhost:3000
          - http://localhost:5173
        allowed_methods:              # HTTP methods allowed in CORS preflight
          - GET
          - POST
          - PUT
          - DELETE
          - OPTIONS

  # ── OpenTelemetry Module ───────────────────────────────────────────────
  # Observability: distributed traces, metrics, and structured logs.
  # Exporter types — traces: "otlp", "memory", "both"
  #                  metrics: "memory", "otlp"
  #                  logs:    "memory", "otlp", "both"
  - class: modules::observability::OtelModule
    config:
      enabled: true                   # Enable tracing (default: false)
      service_name: my-service        # Service name reported to OTEL collector
      service_version: 0.1.0          # Service version (OTEL semantic convention)
      # service_namespace: production # Service namespace (OTEL semantic convention)
      exporter: memory                # Trace exporter: "otlp", "memory", or "both" (default: otlp)
      # endpoint: http://localhost:4317  # OTLP gRPC endpoint (for otlp/both exporters)
      sampling_ratio: 1.0             # 0.0 to 1.0, fraction of traces to sample (1.0 = all)
      memory_max_spans: 10000         # Max spans in memory (for memory/both exporters)
      metrics_enabled: true           # Enable metrics collection (default: false)
      metrics_exporter: memory        # Metrics exporter: "memory" or "otlp" (default: memory)
      # metrics_retention_seconds: 3600  # Metrics retention in seconds (default: 3600)
      # metrics_max_count: 10000      # Max metric data points in memory (default: 10000)
      logs_enabled: true              # Enable structured log storage (default: false)
      logs_exporter: memory           # Logs exporter: "memory", "otlp", or "both" (default: memory)
      logs_max_count: 1000            # Max log entries in memory (default: 1000)
      # logs_retention_seconds: 3600  # Logs retention in seconds (default: 3600)
      # logs_sampling_ratio: 1.0     # Fraction of logs to keep, 0.0-1.0 (default: 1.0)
      # logs_console_output: true    # Also output OTEL logs to console (default: true)
      # level: info                  # Engine log level: trace, debug, info, warn, error
      # format: default              # Log format: "default" (human-readable) or "json"

  # ── Queue Module ───────────────────────────────────────────────────────
  # Message queues for async step-to-step communication via enqueue().
  # Adapters: BuiltinQueueAdapter, RedisAdapter, RabbitMQAdapter
  - class: modules::queue::QueueModule
    config:
      adapter:
        class: modules::queue::BuiltinQueueAdapter  # In-process queue (no external deps)
        # For Redis:  class: modules::queue::RedisAdapter
        #             config: { redis_url: "redis://localhost:6379" }
        # For RabbitMQ: class: modules::queue::RabbitMQAdapter
        #               config: { amqp_url: "amqp://localhost:5672" }

  # ── PubSub Module ─────────────────────────────────────────────────────
  # Internal publish/subscribe messaging between engine components.
  # Adapters: LocalAdapter, RedisAdapter
  - class: modules::pubsub::PubSubModule
    config:
      adapter:
        class: modules::pubsub::LocalAdapter  # In-process pubsub (no external deps)
        # For Redis: class: modules::pubsub::RedisAdapter
        #            config: { redis_url: "redis://localhost:6379" }

  # ── Cron Module ────────────────────────────────────────────────────────
  # Schedules and executes cron-based triggers.
  # Adapters: KvCronAdapter, RedisCronAdapter
  - class: modules::cron::CronModule
    config:
      adapter:
        class: modules::cron::KvCronAdapter  # KV-based scheduler (no external deps)
        # For Redis: class: modules::cron::RedisCronAdapter
        #            config: { redis_url: "redis://localhost:6379" }

  # ── Exec Module ────────────────────────────────────────────────────────
  # Manages the SDK process lifecycle. Watches files and restarts on change.
  - class: modules::shell::ExecModule
    config:
      watch:                          # Glob patterns to watch for hot-reload
        - steps/**/*.ts
        - motia.config.ts
      exec:                           # Commands to run as the SDK process (in order)
        - npx motia dev
        - bun run --enable-source-maps dist/index-dev.js
```

**New -- `motia.config.ts` (auth/hooks):**

```typescript
import type { AuthenticateStream } from 'motia'

export const authenticateStream: AuthenticateStream = async (req, context) => {
  context.logger.info('Authenticating stream', { req })
  return { context: { userId: 'sergio' } }
}
```

### Dev Command

| Old | New |
|---|---|
| `motia dev` | `iii` |
| `motia build` | `motia build` (unchanged) |

### Files to Delete

- `motia-workbench.json`
- `.motia/` directory (auto-generated state) — **Warning:** this will delete any local stream and state data persisted by the old engine; back up first if needed

Note: `motia.config.ts` is **not deleted** -- it is simplified. Remove the `defineConfig` wrapper, all plugin imports, and the `plugins` array. Keep only the authentication hook exports (see the "New" example above).

---

## 2. Module System and Runtime

The new Motia **does not enforce a specific module system or runtime**. You are free to use CommonJS, ESM, Node.js, Bun, or any compatible runtime. The framework adapts to your project's setup.

### Runtime Support

Motia now has first-class support for **Bun** in addition to Node.js. You can choose whichever runtime fits your project:

| Runtime | Dev Command Example | Production Example |
|---|---|---|
| Node.js | `npx motia dev` | `node dist/index-production.js` |
| Bun | `bun run dist/index-dev.js` | `bun run --enable-source-maps dist/index-production.js` |

### Module System

You can use either CommonJS or ESM -- the choice is yours. If you want to adopt ESM (recommended for Bun compatibility and modern tooling), update your project:

**`package.json` -- optionally add:**

```json
{
  "type": "module"
}
```

**`tsconfig.json` -- optionally change:**

```jsonc
{
  "compilerOptions": {
    "module": "ESNext",
    "moduleResolution": "bundler",
    "moduleDetection": "force"
  }
}
```

If you prefer to stay on CommonJS, that works too. Motia does not force a migration.

---

## 3. Steps and Triggers -- Unified Config Model

**This is the most important conceptual change in new Motia: there are no longer separate "step types".** In the old version, you had API steps, Event steps, and Cron steps -- each with its own config type (`ApiRouteConfig`, `EventConfig`, `CronConfig`) and its own `type` field. In the new version, **everything is just a Step**. What used to determine the "type" of a step is now expressed through its **triggers** -- an array of trigger definitions that describe how and when the step is activated.

A single step can have multiple triggers of different kinds (HTTP, queue, cron, state, stream), making it far more flexible than the old one-type-per-step model.

### Config Type Changes

| Old | New |
|---|---|
| `import { ... } from '@motiadev/core'` | `import { ... } from 'motia'` |
| `ApiRouteConfig` | `StepConfig` |
| `EventConfig` | `StepConfig` |
| `CronConfig` | `StepConfig` |
| `type: 'api' | 'event' | 'cron'` | `triggers: [{ type: 'http' | 'queue' | 'cron' | 'state' | 'stream' }]` |
| `emits: ['topic']` | `enqueues: ['topic']` |
| `subscribes: ['topic']` | Moved into trigger: `{ type: 'queue', topic: '...' }` |
| `virtualEmits` | `virtualEnqueues` |
| `virtualSubscribes` | `virtualSubscribes` (unchanged) |

### Handler Type Changes

| Old | New |
|---|---|
| `Handlers['StepName']` | `Handlers<typeof config>` |
| `StepHandler<typeof config>` | `Handlers<typeof config>` |
| `ctx.emit({ topic, data })` | `ctx.enqueue({ topic, data })` |

### Type Safety

The new version uses `as const satisfies StepConfig` for full type inference:

```typescript
// Old
export const config: ApiRouteConfig = {
  type: 'api',
  name: 'MyStep',
  // ...
}
export const handler: Handlers['MyStep'] = async (req, ctx) => { ... }

// New
export const config = {
  name: 'MyStep',
  // ...
  triggers: [{ type: 'http', method: 'GET', path: '/my-step' }],
  enqueues: [],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (req, ctx) => { ... }
```

---

## 4. HTTP Triggers

In the old version these were "API steps" -- a dedicated step type with `type: 'api'`. In the new version, HTTP is just a **trigger type** (`type: 'http'`) on a regular step.

### Before (Old)

```typescript
import { ApiRouteConfig, Handlers } from 'motia'
import { z } from 'zod'

const bodySchema = z.object({
  name: z.string(),
  email: z.string(),
})

export const config: ApiRouteConfig = {
  type: 'api',
  name: 'CreateUser',
  description: 'Create a new user',
  method: 'POST',
  path: '/users',
  bodySchema,
  responseSchema: {
    200: z.object({ id: z.string() }),
    400: z.object({ error: z.string() }),
  },
  emits: ['user-created'],
  flows: ['User Flow'],
  middleware: [coreMiddleware, validateBearerToken],
}

export const handler: Handlers['CreateUser'] = async (req, { emit, logger }) => {
  const { name, email } = req.body

  logger.info('Creating user', { name, email })

  await emit({
    topic: 'user-created',
    data: { name, email },
  })

  return { status: 200, body: { id: 'user-123' } }
}
```

### After (New)

```typescript
import type { Handlers, StepConfig } from 'motia'
import { z } from 'zod'

const bodySchema = z.object({
  name: z.string(),
  email: z.string(),
})

export const config = {
  name: 'CreateUser',
  description: 'Create a new user',
  flows: ['user-flow'],
  triggers: [
    {
      type: 'http',
      method: 'POST',
      path: '/users',
      bodySchema,
      responseSchema: {
        200: z.object({ id: z.string() }),
        400: z.object({ error: z.string() }),
      },
      middleware: [validateBearerToken],
    },
  ],
  enqueues: ['user-created'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (req, { enqueue, logger }) => {
  const { name, email } = req.body

  logger.info('Creating user', { name, email })

  await enqueue({
    topic: 'user-created',
    data: { name, email },
  })

  return { status: 200, body: { id: 'user-123' } }
}
```

### Key Differences

1. `type: 'api'` is now `type: 'http'` inside a trigger object.
2. `method`, `path`, `bodySchema`, `responseSchema`, `middleware` all move inside the trigger.
3. `emits` becomes `enqueues` at the config level.
4. `emit()` becomes `enqueue()` in the handler context.
5. Config type changes from `ApiRouteConfig` to `StepConfig` with `as const satisfies`.
6. In old `emit()` calls, some projects used `type` as the field name (e.g., `emit({ type: 'topic-name', data })`) while others used `topic`. The new `enqueue()` always uses `topic`: `enqueue({ topic: 'topic-name', data })`.

### HTTP Helper Shorthand

The new version provides an `http()` helper for cleaner trigger definitions:

```typescript
import { http } from 'motia'

export const config = {
  name: 'CreateTodo',
  flows: ['todo-app'],
  triggers: [
    http('POST', '/todo', {
      bodySchema: z.object({ description: z.string() }),
      responseSchema: {
        200: todoSchema,
        400: z.object({ error: z.string() }),
      },
    }),
  ],
  enqueues: [],
} as const satisfies StepConfig
```

> **Note:** Both the TypeScript and Python SDKs use `http()` as the primary trigger helper. Both also export `api()` as a deprecated alias — it works identically but should be updated to `http()` for future compatibility.

---

## 5. Queue Triggers (formerly Event Steps)

The concept of "event steps" that subscribe to topics no longer exists as a step type. Instead, subscribing to a topic is now a **queue trigger** on a regular step.

### Before (Old)

```typescript
import { EventConfig, Handlers } from '@motiadev/core'
import { z } from 'zod'

export const config: EventConfig = {
  type: 'event',
  name: 'DeployEnvironment',
  description: 'Creates or updates an environment',
  subscribes: ['deploy-environment-v2'],
  emits: ['deploy-version-v2'],
  input: z.object({
    deploymentId: z.string(),
    envVars: z.record(z.string()),
  }),
  flows: ['Deployment'],
}

export const handler: Handlers['DeployEnvironment'] = async (data, { logger, emit, streams }) => {
  logger.info('Deploying environment', { deploymentId: data.deploymentId })

  // ... business logic ...

  await emit({
    topic: 'deploy-version-v2',
    data: { deploymentId: data.deploymentId },
  })
}
```

### After (New)

```typescript
import type { Handlers, StepConfig } from 'motia'
import { z } from 'zod'

export const config = {
  name: 'DeployEnvironment',
  description: 'Creates or updates an environment',
  flows: ['deployment'],
  triggers: [
    {
      type: 'queue',
      topic: 'deploy-environment-v2',
      input: z.object({
        deploymentId: z.string(),
        envVars: z.record(z.string()),
      }),
    },
  ],
  enqueues: ['deploy-version-v2'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input, { logger, enqueue, streams }) => {
  logger.info('Deploying environment', { deploymentId: input.deploymentId })

  // ... business logic ...

  await enqueue({
    topic: 'deploy-version-v2',
    data: { deploymentId: input.deploymentId },
  })
}
```

### Key Differences

| Old | New |
|---|---|
| `type: 'event'` | `triggers: [{ type: 'queue', topic, input }]` |
| `subscribes: ['topic']` | `topic` field inside trigger |
| `emits: ['topic']` | `enqueues: ['topic']` |
| `input: schema` | `input: schema` inside trigger (or wrap with `jsonSchema()`) |
| `infrastructure: {...}` at config root | `config: {...}` inside the queue trigger |
| `emit({ topic, data })` | `enqueue({ topic, data })` |
| `emit({ type: 'topic' })` (some old projects) | `enqueue({ topic: 'topic' })` (field key standardized to `topic`) |
| Handler receives `data` directly | Handler receives `input` directly |

### Using `jsonSchema()` Wrapper

When the input schema needs JSON schema conversion for the engine, use the `jsonSchema()` wrapper:

```typescript
import { jsonSchema } from 'motia'

triggers: [
  {
    type: 'queue',
    topic: 'notification',
    input: jsonSchema(
      z.object({
        email: z.string(),
        templateId: z.string(),
      })
    ),
  },
]
```

---

## 6. Cron Triggers

### Before (Old)

```typescript
import { CronConfig, Handlers } from '@motiadev/core'

export const config: CronConfig = {
  type: 'cron',
  name: 'DailyMetricsCollection',
  description: 'Collects metrics daily at midnight',
  cron: '0 5 * * *',
  emits: ['collect-metrics'],
  flows: ['Metrics Collection Flow'],
}

export const handler: Handlers['DailyMetricsCollection'] = async ({ logger, emit }) => {
  logger.info('Collecting metrics')

  await emit({
    topic: 'collect-metrics',
    data: { targetDate: new Date().toISOString() },
  })
}
```

### After (New)

```typescript
import type { Handlers, StepConfig } from 'motia'

export const config = {
  name: 'DailyMetricsCollection',
  description: 'Collects metrics daily at midnight',
  flows: ['metrics-collection-flow'],
  triggers: [
    {
      type: 'cron',
      expression: '0 0 5 * * *',
    },
  ],
  enqueues: ['collect-metrics'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input, { logger, enqueue }) => {
  logger.info('Collecting metrics')

  await enqueue({
    topic: 'collect-metrics',
    data: { targetDate: new Date().toISOString() },
  })
}
```

### Key Differences

| Old | New |
|---|---|
| `type: 'cron'` at config root | `triggers: [{ type: 'cron', expression }]` |
| `cron: '0 5 * * *'` (5-field) | `expression: '0 0 5 * * *'` (6-field: prepend seconds; 7th year field optional) |
| Handler: `async ({ logger, emit })` | Handler: `async (input, { logger, enqueue })` |
| `emit()` | `enqueue()` |

### Cron Expression Format

The new engine uses a 6-field cron expression (7th year field is optional):

```
┌──────────── second (0-59)
│ ┌────────── minute (0-59)
│ │ ┌──────── hour (0-23)
│ │ │ ┌────── day of month (1-31)
│ │ │ │ ┌──── month (1-12)
│ │ │ │ │ ┌── day of week (0-6, Sun=0)
│ │ │ │ │ │ ┌ year (optional)
│ │ │ │ │ │ │
* * * * * * *
```

**Conversion examples:**

| Old (5-field) | New (6-field) | Meaning |
|---|---|---|
| `0 5 * * *` | `0 0 5 * * *` | Daily at 5:00 AM |
| `0 2 * * *` | `0 0 2 * * *` | Daily at 2:00 AM |
| `*/5 * * * *` | `0 */5 * * * *` | Every 5 minutes |
| `0 0 * * 0` | `0 0 0 * * 0` | Weekly on Sunday at midnight |

---

## 7. Streams

Stream definitions remain similar but the access API has changed.

### Stream Config

**Old:**

```typescript
import { StreamConfig } from '@motiadev/core'
import { z } from 'zod'

export const config: StreamConfig = {
  name: 'deployment',
  baseConfig: { storageType: 'default' },
  schema: z.object({
    id: z.string(),
    status: z.enum(['pending', 'progress', 'completed', 'failed']),
    message: z.string().optional(),
  }),
}
```

**New:**

```typescript
import type { StreamConfig } from 'motia'
import { z } from 'zod'

export const config: StreamConfig = {
  name: 'deployment',
  baseConfig: { storageType: 'default' },
  schema: z.object({
    id: z.string(),
    status: z.enum(['pending', 'progress', 'completed', 'failed']),
    message: z.string().optional(),
  }),

  // New: lifecycle hooks (optional)
  onJoin: async (subscription, context, authContext) => {
    context.logger.info('Client joined stream', { subscription, authContext })
    return { unauthorized: false }
  },
  onLeave: async (subscription, context, authContext) => {
    context.logger.info('Client left stream', { subscription, authContext })
  },
}
```

### Stream Operations API

| Operation | Old | New |
|---|---|---|
| Get | `streams.name.get(id, key)` | `streams.name.get(groupId, id)` |
| Set | `streams.name.set(id, key, value)` | `streams.name.set(groupId, id, value)` |
| Update | N/A | `streams.name.update(groupId, id, UpdateOp[])` |
| Delete | `streams.name.delete(id, key)` | `streams.name.delete(groupId, id)` |

The parameter naming changed from `(id, key)` to `(groupId, id)` to better reflect the data model: a stream is partitioned by groups, and within each group items are identified by id.

### Atomic Updates with `UpdateOp`

The new version supports atomic update operations:

```typescript
import type { UpdateOp } from 'motia'

await streams.deployment.update('merge-groups', traceId, [
  { type: 'increment', path: 'completedSteps', by: 1 },
  { type: 'set', path: 'status', value: 'progress' },
  { type: 'decrement', path: 'retries', by: 1 },
])
```

**Available operations:**

| Type | Fields | Description |
|---|---|---|
| `set` | `path`, `value` | Set a field to a value (overwrite) |
| `merge` | `path` (optional), `value` | Merge an object into the existing value (object-only) |
| `increment` | `path`, `by` | Increment a numeric field |
| `decrement` | `path`, `by` | Decrement a numeric field |
| `remove` | `path` | Remove a field entirely |

### Migration Example

**Old:**

```typescript
const streamData = await streams.deployment.get(deploymentId, 'data')
streamData.status = 'completed'
streamData.message = 'Done'
await streams.deployment.set(deploymentId, 'data', streamData)
```

**New:**

```typescript
await streams.deployment.update('data', deploymentId, [
  { type: 'set', path: 'status', value: 'completed' },
  { type: 'set', path: 'message', value: 'Done' },
])
```

### Stream Triggers (New)

Steps can now react to stream changes. The handler receives a `StreamWrapperMessage` with the following shape:

```typescript
type StreamWrapperMessage<TStreamData> = {
  type: 'stream'
  timestamp: number
  streamName: string
  groupId: string
  id?: string
  event: StreamCreate<TStreamData> | StreamUpdate<TStreamData> | StreamDelete<TStreamData> | StreamEvent
}
```

Where the `event` field contains one of:
- `{ type: 'create', data: TStreamData }` -- a new item was created
- `{ type: 'update', data: TStreamData }` -- an existing item was updated
- `{ type: 'delete', data: TStreamData }` -- an item was deleted
- `{ type: 'event', data: { type: string, data: TEventData } }` -- a custom event

Define a stream trigger with a `condition` to filter which stream messages activate the step:

```typescript
triggers: [
  {
    type: 'stream',
    streamName: 'deployment',
    groupId: 'data',
    condition: (input: StreamWrapperMessage) => input.event.type === 'update',
  },
]
```

---

## 8. State

State provides key-value storage grouped by a namespace. The core `get`, `set`, and `list` operations remain the same as before. The new version introduces two important additions: **atomic updates** via the `update` method, and **state triggers**.

### Existing API (unchanged)

```typescript
// Set a value
await ctx.state.set('orders', orderId, orderData)

// Get a value
const order = await ctx.state.get<Order>('orders', orderId)

// List all values in a group
const allOrders = await ctx.state.list<Order>('orders')
```

### New: Atomic Updates with `update()`

Instead of read-modify-write patterns, you can now perform atomic updates on state entries using `UpdateOp[]`:

```typescript
await ctx.state.update<Order>('orders', orderId, [
  { type: 'increment', path: 'completedSteps', by: 1 },
  { type: 'set', path: 'status', value: 'shipped' },
  { type: 'decrement', path: 'retries', by: 1 },
])
```

This is the same `UpdateOp` interface used in streams (see [Streams](#7-streams)). It eliminates race conditions that can occur with manual get-then-set patterns.

**Available operations:**

| Type | Fields | Description |
|---|---|---|
| `set` | `path`, `value` | Set a field to a value (overwrite) |
| `merge` | `path` (optional), `value` | Merge an object into the existing value (object-only) |
| `increment` | `path`, `by` | Increment a numeric field |
| `decrement` | `path`, `by` | Decrement a numeric field |
| `remove` | `path` | Remove a field entirely |

### New: State Triggers

**This is a brand new feature.** Steps can now react to state changes by using a `state` trigger. The trigger includes a `condition` function that filters which state changes should activate the step:

```typescript
import type { StateTriggerInput } from 'motia'

export const config = {
  name: 'OnAllStepsComplete',
  triggers: [
    {
      type: 'state',
      condition: (input: StateTriggerInput<MyType>) => {
        return (
          input.group_id === 'tasks' &&
          !!input.new_value &&
          input.new_value.totalSteps === input.new_value.completedSteps
        )
      },
    },
  ],
  flows: ['my-flow'],
} as const satisfies StepConfig
```

The handler receives the state change event as its first argument, including `new_value`, `old_value`, `item_id`, and `group_id`. This enables powerful reactive patterns -- for example, triggering a step when a parallel merge completes, without polling or manual coordination.

---

## 9. Middleware

### Old Approach

In the old version, middleware was defined as `ApiMiddleware` functions and attached to step configs:

```typescript
// src/middleware/bearerToken.middleware.ts
import { ApiMiddleware } from '@motiadev/core'

export const validateBearerToken: ApiMiddleware = async (req, ctx, next) => {
  const authToken = req.headers['authorization'] as string
  if (!authToken) {
    return { status: 401, body: { error: 'Unauthorized' } }
  }
  // validate token...
  req.tokenInfo = decoded
  return next()
}

// In step config:
export const config: ApiRouteConfig = {
  type: 'api',
  name: 'GetUser',
  middleware: [coreMiddleware, validateBearerToken],
  // ...
}
```

### New Approach

The `middleware` field has moved from the config root **into the HTTP trigger object**:

```typescript
export const config = {
  name: 'GetUser',
  flows: ['users'],
  triggers: [
    {
      type: 'http',
      method: 'GET',
      path: '/users',
      middleware: [validateBearerToken],
    },
  ],
  enqueues: [],
} as const satisfies StepConfig
```

**Stream authentication** is configured separately in `motia.config.ts` via `authenticateStream`.

You can also use shared utility functions called directly within handlers as an alternative:

```typescript
// Alternative: handler-level auth
export async function requireAuth(request: ApiRequest<any>): Promise<TokenData> {
  const authToken = request.headers['authorization'] as string
  if (!authToken) {
    throw new HttpError(401, 'Unauthorized')
  }
  const [, token] = authToken.split(' ')
  return jwt.verify(token, env.JWT_SECRET) as TokenData
}

export const handler: Handlers<typeof config> = async (request, { logger }) => {
  const tokenData = await requireAuth(request)
  // ... rest of handler
}
```

---

## 10. New Features

### Multi-Trigger Steps

A single step can now respond to multiple trigger types:

```typescript
export const config = {
  name: 'ProcessOrder',
  flows: ['orders'],
  triggers: [
    { type: 'queue', topic: 'order.created', input: orderSchema },
    { type: 'http', method: 'POST', path: '/orders/manual', bodySchema: orderSchema },
    { type: 'cron', expression: '* * * * *' },
  ],
  enqueues: ['order.processed'],
} as const satisfies StepConfig
```

### The `step()` Helper

For multi-trigger steps, the `step()` helper provides `ctx.getData()` and `ctx.match()`:

```typescript
import { http, queue, step } from 'motia'

export const stepConfig = {
  name: 'ProcessOrder',
  flows: ['orders'],
  triggers: [
    queue('order.created', { input: orderSchema }),
    http('POST', '/orders', { bodySchema: orderSchema }),
  ],
  enqueues: ['notification'],
}

export const { config, handler } = step(stepConfig, async (input, ctx) => {
  // ctx.getData() returns the data regardless of trigger type
  const data = ctx.getData()

  // ctx.match() for trigger-specific handling
  return ctx.match({
    http: async (request) => {
      return { status: 200, body: { success: true } }
    },
    queue: async (queueInput) => {
      ctx.logger.info('Processing from queue', { queueInput })
    },
  })
})
```

### Conditional Triggers

Triggers can include a `condition` function that determines whether the step should execute:

```typescript
triggers: [
  {
    type: 'queue',
    topic: 'order.created',
    input: orderSchema,
    condition: (input, ctx) => {
      return input.amount > 1000  // Only process high-value orders
    },
  },
  {
    type: 'http',
    method: 'POST',
    path: '/orders/manual',
    bodySchema: orderSchema,
    condition: (input, ctx) => {
      if (ctx.trigger.type !== 'http') return false
      return input.body.user.verified === true
    },
  },
]
```

### Helper Functions

Shorthand helpers for creating triggers:

```typescript
import { http, queue } from 'motia'

triggers: [
  http('POST', '/todo', { bodySchema: schema, responseSchema: { 200: schema } }),
  queue('process-todo', { input: schema }),
]
```

---

## 11. Migration Checklist

### Project Setup

- [ ] Install the iii engine from [https://iii.dev](https://iii.dev)
- [ ] Create `config.yaml` with module definitions (stream, state, api, queue, cron, exec)
- [ ] Create `motia.config.ts` for authentication hooks (if needed)
- [ ] Simplify `motia.config.ts`: remove `defineConfig`, all plugin imports, and the `plugins` array; keep only auth hook exports
- [ ] Delete `motia-workbench.json`
- [ ] Delete `.motia/` directory — **Warning:** this will delete any local stream and state data persisted by the old engine; back up first if needed
- [ ] Update dev script from `motia dev` to `iii`
- [ ] Choose your runtime (Node.js or Bun) and module system (CommonJS or ESM)

### Steps

- [ ] Replace all `@motiadev/core` imports with `motia`
- [ ] Replace all `ApiRouteConfig` / `EventConfig` / `CronConfig` imports with `StepConfig`
- [ ] Convert all step configs to use `triggers[]` and `enqueues[]`
- [ ] Add `as const satisfies StepConfig` to all configs
- [ ] Replace `Handlers['StepName']` with `Handlers<typeof config>`
- [ ] Rename all `emit()` calls to `enqueue()`
- [ ] Rename all `emits` config fields to `enqueues`
- [ ] Move `subscribes` into queue triggers
- [ ] Move `method`, `path`, `bodySchema`, `responseSchema`, `middleware` into HTTP triggers
- [ ] Move `infrastructure` from config root into queue triggers as `config`
- [ ] Change `type: 'api'` to `type: 'http'` in all triggers
- [ ] Move `cron` into cron triggers as `expression` (prepend seconds; 7th year field is optional)
- [ ] Remove `type` field from config root
- [ ] Remove `middleware` field from all step configs
- [ ] Replace `virtualEmits` with `virtualEnqueues` (format changes from `[{ topic, label }]` to `['topic']`)
- [ ] Keep `virtualSubscribes` as-is (no rename needed)

### Streams

- [ ] Update stream access calls: `get(id, key)` to `get(groupId, id)`
- [ ] Update stream access calls: `set(id, key, value)` to `set(groupId, id, value)`
- [ ] Replace read-modify-write patterns with `update(groupId, id, UpdateOp[])` where possible
- [ ] Add `onJoin` / `onLeave` hooks to stream configs if real-time subscription auth is needed

### State

- [ ] Adopt `state.update()` with `UpdateOp[]` to replace manual get-then-set patterns
- [ ] Consider using state triggers for reactive workflows

### Middleware

- [ ] Move `middleware` arrays from config root into the corresponding HTTP trigger objects
- [ ] Alternatively, extract authentication logic into shared utility functions called in handlers

### Cron Expressions

- [ ] Convert all 5-field cron expressions to 6-field format (prepend seconds; 7th year field is optional)
- [ ] Rename `cron` field to `expression` inside trigger objects

### Python (if applicable)

- [ ] Install `motia` as a standalone Python package (npm/Node.js no longer required!)
- [ ] Add a separate ExecModule entry in `config.yaml` for the Python runtime
- [ ] Refer to the dedicated Python migration guide for step-level changes

### Workbench and Plugins

- [ ] Delete `motia-workbench.json`
- [ ] Remove any `.ui.step.ts` or noop step files used exclusively for workbench rendering
- [ ] Remove any workbench plugin code (React/JSX components for workbench panels)
- [ ] Familiarize with the iii Console as the replacement for the Workbench
- [ ] Remove `@motiadev/workbench` and `@motiadev/core` from `package.json` dependencies (replaced by `motia`)

---

## 12. Python Runtime

**This is a major architectural change.** In the old Motia, Python steps were managed by the same Node.js-based Motia runtime. Python files were executed as child processes spawned by the Node runtime, meaning **Python developers previously needed Node.js and npm installed** to use Motia at all.

In the new Motia, **runtimes are fully independent**. There is a dedicated **Motia Python** SDK (`motia-py`) that runs as its own standalone process, communicating directly with the iii engine. Python developers no longer need Node.js, npm, or any JavaScript tooling whatsoever.

### What Changed

| Aspect | Old | New |
|---|---|---|
| Python execution | Spawned as child process by Node runtime | Independent process managed by iii engine |
| Node.js required for Python? | Yes | **No** |
| SDK | Single `motia` npm package handled both | Separate `motia-py` (Python) and `motia` (Node) packages |
| Configuration | Shared with Node steps | Own `config.yaml` ExecModule entry pointing to the Python process |
| File naming | `*_step.py` | `*_step.py` (unchanged) |
| Package manager | pip / poetry | `uv` (recommended) |

> **Recommended migration order:**
> 1. Set up your Python project (`pyproject.toml` with `uv`) — see [Python Project Setup](#python-project-setup) below
> 2. Add the Python ExecModule entry in `config.yaml` — see [Configuration](#1-configuration) and [Module System](#2-module-system-and-runtime) for full `config.yaml` structure
> 3. Rename step files (`*_step.py` → `*_step.py`)
> 4. Migrate step configs and handlers one at a time (use the subsections below as reference)
> 5. Verify with the [Python Migration Checklist](#python-migration-checklist) at the end of this section

### For Mixed Projects (Node + Python)

If your project has both Node and Python steps, you now configure **separate ExecModule entries** in `config.yaml` -- one for each runtime:

```yaml
modules:
  - class: modules::shell::ExecModule
    config:
      watch:                          # Glob patterns to watch for hot-reload
        - steps/**/*.ts
        - motia.config.ts
      exec:                           # Commands to run as the SDK process (in order)
        - npx motia dev
        - bun run --enable-source-maps dist/index-dev.js

  - class: modules::shell::ExecModule
    config:
      watch:                          # Glob patterns to watch for hot-reload
        - steps/**/*.py
      exec:                           # Commands to run as the SDK process (in order)
        - uv run motia dev --dir steps
```

> For the complete `config.yaml` structure including stream, state, API, queue, and cron adapter modules, see [Section 1: Configuration](#1-configuration) and [Section 2: Module System and Runtime](#2-module-system-and-runtime).

### Python Project Setup

Create a `pyproject.toml` in your project root:

```toml
[project]
name = "my-motia-project"
version = "0.1.0"
requires-python = ">=3.10"
dependencies = [
  "motia[otel]==1.0.0rc17",
  "iii-sdk==0.2.0",
  "pydantic>=2.0",
]

[project.optional-dependencies]
dev = ["pytest>=8.0.0"]

[tool.uv]
package = false
```

> **Migrating from `requirements.txt`:** Move your existing dependencies from `requirements.txt` into the `dependencies` list in `pyproject.toml`. For example, if your `requirements.txt` has `openai>=1.40.0` and `httpx>=0.27.0`, add them alongside the Motia packages:
> ```toml
> dependencies = [
>   "motia[otel]==1.0.0rc17",
>   "iii-sdk==0.2.0",
>   "pydantic>=2.0",
>   # Your existing dependencies:
>   "openai>=1.40.0",
>   "httpx>=0.27.0",
> ]
> ```
> Then delete `requirements.txt` — `uv sync` will install everything from `pyproject.toml`.

### Python Step Migration -- Quick Reference

| Concern | Old | New |
|---|---|---|
| Config type field | `"type": "api"` / `"event"` / `"cron"` | Removed — use `triggers` list |
| API trigger | `"method": "POST", "path": "/foo"` at config root | `http("POST", "/foo")` in `triggers` |
| Event trigger | `"subscribes": ["topic"]` at config root | `queue("topic", input=schema)` in `triggers` |
| Cron trigger | `"cron": "0 5 * * *"` at config root | `cron("0 0 5 * * *")` in `triggers` (6-field: prepend seconds; 7th year field optional) |
| State trigger | N/A (new in v1.0) | `state(condition=fn)` in `triggers` |
| Stream trigger | N/A (new in v1.0) | `stream("streamName")` in `triggers` |
| Enqueue config (was `emits`) | `"emits": ["topic"]` | `"enqueues": ["topic"]` |
| Enqueue function (was `emit`) | `context.emit({"topic": ..., "data": ...})` | `ctx.enqueue({"topic": ..., "data": ...})` |
| Input schema location | `"input": Schema.model_json_schema()` at config root | `queue("topic", input=Schema.model_json_schema())` inside trigger |
| Body schema location | `"bodySchema": Schema.model_json_schema()` at config root | `http("POST", "/foo", body_schema=Schema.model_json_schema())` inside trigger |
| File naming | `*_step.py` | `*_step.py` (unchanged) |
| State list | `context.state.get_group("group")` | `ctx.state.list("group")` |
| Streams | `ctx.streams.streamName.get(group_id, id)` | `Stream("name")` module-level declaration |
| Logger | `context.logger` | `ctx.logger` |
| Trace ID | `context.trace_id` | `ctx.trace_id` |
| Path params | `req.get("pathParams", {}).get("id")` | `request.path_params["id"]` |
| Query params | `req.get("queryParams", {})` | `request.query_params` |
| Headers | `req.get("headers", {})` | `request.headers` |
| Labeled enqueues | `"emits": [{"topic": "x", "label": "y", "conditional": True}]` | `"enqueues": [{"topic": "x", "label": "y", "conditional": True}]` (same format, key renamed) |

> **Note:** Some older projects used `"type"` instead of `"topic"` as the key in `emit()` calls (e.g., `context.emit({"type": "topic-name", "data": {...}})`). The new `enqueue()` always uses `"topic"`.

<!-- -->

> **Note on parameter names:** The migration examples use `ctx` and `input_data` as handler parameter names by convention, but any valid Python names work (e.g., `context`, `data`). The framework identifies handlers by function name (`handler`) and argument count, not parameter names.

### API Steps

#### Before (Old)

```python
# steps/petstore/api_step.py
from pydantic import BaseModel

class RequestBody(BaseModel):
    name: str
    category: str

class Bill(BaseModel):
    id: str
    name: str
    category: str

config = {
    "type": "api",
    "name": "Bill Classifier API Step",
    "flows": ["classify-bill"],
    "method": "POST",
    "path": "/classify-bill",
    "bodySchema": RequestBody.model_json_schema(),
    "responseSchema": {200: Bill.model_json_schema()},
    "emits": ["bill-created"],
}

async def handler(req, context):
    body = req.get("body", {})
    context.logger.info("Processing API Step", {"body": body})

    new_bill = {"id": "bill-123", "name": body["name"], "category": body["category"]}

    await context.emit({"topic": "bill-created", "data": new_bill})

    return {"status": 200, "body": {**new_bill, "traceId": context.trace_id}}
```

#### After (New)

```python
# steps/petstore/classify_bill_api_step.py
from typing import Any

from motia import ApiRequest, ApiResponse, FlowContext, http
from pydantic import BaseModel


class RequestBody(BaseModel):
    name: str
    category: str


class Bill(BaseModel):
    id: str
    name: str
    category: str


config = {
    "name": "BillClassifierAPI",
    "flows": ["classify-bill"],
    "triggers": [http("POST", "/classify-bill")],
    "enqueues": ["bill-created"],
}


async def handler(request: ApiRequest[Any], ctx: FlowContext[Any]) -> ApiResponse[dict]:
    body = request.body
    ctx.logger.info("Processing API Step", {"body": body})

    new_bill = {"id": "bill-123", "name": body["name"], "category": body["category"]}

    await ctx.enqueue({"topic": "bill-created", "data": new_bill})

    return ApiResponse(status=200, body={**new_bill, "traceId": ctx.trace_id})
```

#### Key Differences
1. `"type": "api"` removed -- replaced by `http()` trigger in `triggers` list.
2. `"method"` and `"path"` move from config root into the `http()` call.
3. `"emits"` becomes `"enqueues"`.
4. `context.emit()` becomes `ctx.enqueue()`.
5. Handler receives typed `ApiRequest` and returns `ApiResponse` instead of raw dicts.
6. `req.get("body", {})` becomes `request.body`.
7. File naming convention changes from `*_step.py` to `*_step.py` (e.g., `classify_bill_api_step.py`).
8. `req.get("pathParams", {}).get("id")` becomes `request.path_params["id"]`.
9. `req.get("queryParams", {})` becomes `request.query_params`.

#### API Trigger Advanced Options

The `http()` helper supports additional keyword arguments:

```python
from motia import http, QueryParam

http("POST", "/orders",
    body_schema=OrderInput.model_json_schema(),
    response_schema={200: OrderResponse.model_json_schema()},
    query_params=[QueryParam(name="filter", description="Filter criteria")],
    middleware=[auth_middleware],
    condition=is_authorized,
)
```

#### `ApiRequest` and `ApiResponse` Fields

The `ApiRequest` object replaces the old raw `req` dict. Here are all available fields:

| Field | Type | Old equivalent | Description |
|---|---|---|---|
| `request.body` | `TBody \| None` | `req.get("body", {})` | Parsed request body |
| `request.path_params` | `dict[str, str]` | `req.get("pathParams", {})` | URL path parameters (e.g., `/users/:id` → `{"id": "123"}`) |
| `request.query_params` | `dict[str, str \| list[str]]` | `req.get("queryParams", {})` | URL query parameters |
| `request.headers` | `dict[str, str \| list[str]]` | `req.get("headers", {})` | HTTP request headers |

The `ApiResponse` object replaces the old raw return dict:

| Field | Type | Old equivalent | Description |
|---|---|---|---|
| `status` | `int` | `"status"` key | HTTP status code |
| `body` | `Any` | `"body"` key | Response body |
| `headers` | `dict[str, str]` | N/A (new) | Response headers (optional) |

**Example — GET endpoint with path parameters:**

```python
# Old:
async def handler(req, context):
    ingestion_id = req.get("pathParams", {}).get("ingestion_id")
    return {"status": 200, "body": {"id": ingestion_id}}

# New:
async def handler(request: ApiRequest[Any], ctx: FlowContext[Any]) -> ApiResponse[dict]:
    ingestion_id = request.path_params["ingestion_id"]
    return ApiResponse(status=200, body={"id": ingestion_id})
```

> **Note:** Path parameter syntax in routes is unchanged — use `:param` style (e.g., `http("GET", "/ingest/:ingestion_id")`).

#### Python Middleware

Middleware functions can be attached to API triggers via the `middleware` parameter:

```python
from typing import Any, Awaitable, Callable

from motia import ApiRequest, ApiResponse, FlowContext


async def auth_middleware(
    request: ApiRequest[Any],
    ctx: FlowContext[Any],
    next: Callable[[], Awaitable[ApiResponse[Any]]],
) -> ApiResponse[Any]:
    token = request.headers.get("authorization", "")
    if not token.startswith("Bearer "):
        return ApiResponse(status=401, body={"error": "Unauthorized"})
    return await next()
```

#### Infrastructure Configuration

Steps can configure resource limits and queue behavior:

```python
config = {
    "name": "HeavyComputation",
    "triggers": [
        {
            "type": "queue",
            "topic": "heavy-job",
            "config": {"type": "fifo", "maxRetries": 5},
        }
    ],
}
```

### Queue (Event) Steps

#### Before (Old)

```python
# steps/petstore/process_food_order_step.py
from pydantic import BaseModel

class Bill(BaseModel):
    id: str
    name: str
    category: str

config = {
    "type": "event",
    "name": "Classify Bill Step",
    "flows": ["classify-bill"],
    "subscribes": ["bill-created"],
    "emits": ["notification"],
    "input": Bill.model_json_schema(),
}

async def handler(input_data, context):
    context.logger.info("Processing bill", {"input": input_data})

    await context.emit({
        "topic": "notification",
        "data": {
            "email": "test@test.com",
            "templateId": "bill-classified",
            "templateData": {"billId": input_data["id"]},
        },
    })
```

#### After (New)

```python
# steps/petstore/process_food_order_step.py
from typing import Any

from motia import FlowContext, queue
from pydantic import BaseModel


class Bill(BaseModel):
    id: str
    name: str
    category: str


config = {
    "name": "ClassifyBill",
    "flows": ["classify-bill"],
    "triggers": [queue("bill-created", input=Bill.model_json_schema())],
    "enqueues": ["notification"],
}


async def handler(input_data: dict[str, Any], ctx: FlowContext[Any]) -> None:
    ctx.logger.info("Processing bill", {"input": input_data})

    await ctx.enqueue({
        "topic": "notification",
        "data": {
            "email": "test@test.com",
            "templateId": "bill-classified",
            "templateData": {"billId": input_data["id"]},
        },
    })
```

#### Key Differences

1. `"type": "event"` removed -- replaced by `queue()` trigger.
2. `"subscribes": ["bill-created"]` moves into `queue("bill-created", ...)`.
3. `"input": schema` moves from config root into the `queue()` call as a keyword argument.
4. `"emits"` becomes `"enqueues"`.
5. `context.emit()` becomes `ctx.enqueue()`.
6. Handler input is a plain `dict` — use `input_data["key"]` or `input_data.get("key")` instead of attribute access (`args.key`). For type-safe access, validate with Pydantic: `payload = MyModel.model_validate(input_data)`.

**Labeled enqueues** — The `"enqueues"` field supports both simple strings and dicts with metadata, exactly as the old `"emits"` did:

```python
# Simple format
"enqueues": ["notification", "audit-log"]

# Labeled format (same structure as old "emits")
"enqueues": [
    {"topic": "notification", "label": "Send email notification"},
    {"topic": "audit-log", "label": "Log to audit trail", "conditional": True},
]
```

**Subscribing to multiple topics** — a single step can listen to multiple queue topics by adding multiple triggers:

```python
config = {
    "name": "ChessGameMoved",
    "flows": ["chess"],
    "triggers": [
        queue("chess-game-moved", input=InputSchema.model_json_schema()),
        queue("chess-game-created", input=InputSchema.model_json_schema()),
    ],
    "enqueues": ["ai-move"],
}
```

### Cron Steps

#### Before (Old)

```python
# steps/petstore/state_audit_cron_step.py
config = {
    "type": "cron",
    "cron": "0 0 * * 1",
    "name": "StateAuditJob",
    "emits": ["notification"],
    "flows": ["classify-bill"],
}

async def handler(context):
    state_value = await context.state.get_group("orders_python")
    context.logger.info("Auditing state", {"count": len(state_value)})

    await context.emit({
        "topic": "notification",
        "data": {"count": len(state_value)},
    })
```

#### After (New)

```python
# steps/petstore/state_audit_cron_step.py
from typing import Any

from motia import FlowContext, cron


config = {
    "name": "StateAuditJob",
    "flows": ["classify-bill"],
    "triggers": [cron("0 0 0 * * 1")],
    "enqueues": ["notification"],
}


async def handler(input_data: None, ctx: FlowContext[Any]) -> None:
    state_value = await ctx.state.list("orders_python")
    ctx.logger.info("Auditing state", {"count": len(state_value)})

    await ctx.enqueue({
        "topic": "notification",
        "data": {"count": len(state_value)},
    })
```

#### Key Differences

1. `"type": "cron"` removed -- replaced by `cron()` trigger.
2. `"cron": "0 0 * * 1"` (5-field) becomes `cron("0 0 0 * * 1")` (6-field: prepend seconds). The 7th year field is optional — see [Cron Expression Format](#cron-expression-format) in Section 6 for the full format diagram.
3. Handler signature changes from `async def handler(context)` (single arg) to `async def handler(input_data, ctx)` (two args -- `input_data` is `None` for cron).
4. `context.state.get_group()` becomes `ctx.state.list()`.
5. `context.emit()` becomes `ctx.enqueue()`.

### State Trigger Steps

State triggers fire when state data changes. This is a **new trigger type** — the old Motia did not have state-triggered steps.

```python
# steps/users/on_user_change_step.py
from typing import Any

from motia import FlowContext, StateTriggerInput, state


config = {
    "name": "OnUserStateChange",
    "description": "React to user state changes",
    "triggers": [
        state(condition=lambda input, ctx: input.group_id == "users"),
    ],
    "enqueues": ["user.status.changed"],
    "flows": ["user-management"],
}


async def handler(input_data: StateTriggerInput, ctx: FlowContext[Any]) -> None:
    ctx.logger.info("User state changed", {
        "group_id": input_data.group_id,
        "item_id": input_data.item_id,
    })

    old_status = input_data.old_value.get("status") if isinstance(input_data.old_value, dict) else None
    new_status = input_data.new_value.get("status") if isinstance(input_data.new_value, dict) else None

    await ctx.enqueue({
        "topic": "user.status.changed",
        "data": {
            "user_id": input_data.item_id,
            "old_status": old_status,
            "new_status": new_status,
        },
    })
```

#### Key Differences

1. The `state()` trigger accepts an optional `condition` function to filter which state changes to react to.
2. The handler receives a `StateTriggerInput` with fields: `group_id`, `item_id`, `old_value`, `new_value`.
3. Import `state` (trigger helper) and `StateTriggerInput` (input type) from `motia`.

### Stream Trigger Steps

Stream triggers fire when stream data is created, updated, or deleted. This is a **new trigger type** — distinct from using `Stream("name")` for CRUD operations.

```python
# steps/todos/on_todo_event_step.py
from typing import Any

from motia import FlowContext, StreamTriggerInput, stream


config = {
    "name": "OnTodoStreamEvent",
    "description": "React to todo stream events",
    "triggers": [
        stream("todo"),
    ],
    "enqueues": ["todo.processed"],
    "flows": ["todo-app"],
}


async def handler(input_data: StreamTriggerInput, ctx: FlowContext[Any]) -> None:
    ctx.logger.info("Stream event", {
        "stream_name": input_data.stream_name,
        "group_id": input_data.group_id,
        "item_id": input_data.id,
        "event_type": input_data.event.type,
    })

    if input_data.event.type == "create":
        ctx.logger.info(f"New todo created: {input_data.id}")
    elif input_data.event.type == "update":
        ctx.logger.info(f"Todo updated: {input_data.id}")
    elif input_data.event.type == "delete":
        ctx.logger.info(f"Todo deleted: {input_data.id}")

    await ctx.enqueue({
        "topic": "todo.processed",
        "data": {"todo_id": input_data.id, "event_type": input_data.event.type},
    })
```

#### Key Differences

1. The `stream()` trigger accepts `stream_name` (required), plus optional `group_id`, `item_id`, and `condition` to filter events.
2. The handler receives a `StreamTriggerInput` with fields: `stream_name`, `group_id`, `id`, `event` (which has `type` and `data`).
3. Event types are `"create"`, `"update"`, or `"delete"`.
4. Import `stream` (trigger helper) and `StreamTriggerInput` (input type) from `motia`.

### Multi-Trigger Steps

A single step can have multiple triggers of different types. The `ctx.match()` method dispatches to the correct handler based on which trigger fired:

```python
# steps/greetings/summary_step.py
from typing import Any

from motia import ApiRequest, ApiResponse, FlowContext, Stream, http, cron

greetings_stream: Stream[dict[str, Any]] = Stream("greetings")

config = {
    "name": "GreetingsSummary",
    "description": "Summarize greetings via API or every 5 seconds",
    "triggers": [
        http("GET", "/greetings/summary"),
        cron("*/5 * * * * *"),
    ],
    "enqueues": [],
}


async def handler(input_data: Any, ctx: FlowContext[Any]) -> Any:
    async def _api_handler(request: ApiRequest[Any]) -> ApiResponse[dict]:
        greetings = await greetings_stream.get_group("default")
        return ApiResponse(status=200, body={"count": len(greetings), "greetings": greetings})

    async def _cron_handler() -> None:
        greetings = await greetings_stream.get_group("default")
        ctx.logger.info("Greetings summary (cron)", {"count": len(greetings)})

    return await ctx.match({
        "http": _api_handler,
        "cron": _cron_handler,
    })
```

#### `ctx.match()` Handler Signatures

| Key | Handler signature | Receives |
|---|---|---|
| `"queue"` | `async (input) -> None` | Queue data |
| `"http"` (or `"api"`) | `async (request) -> ApiResponse` | `ApiRequest` object |
| `"cron"` | `async () -> None` | Nothing |
| `"state"` | `async (input) -> Any` | `StateTriggerInput` |
| `"stream"` | `async (input) -> Any` | `StreamTriggerInput` |
| `"default"` | `async (input) -> Any` | Raw input (fallback) |

#### Trigger Introspection

The `ctx.trigger` attribute (`TriggerInfo`) provides metadata about which trigger fired:

```python
ctx.trigger.type        # "http", "queue", "cron", "state", "stream"
ctx.trigger.topic       # queue topic (queue triggers only)
ctx.trigger.path        # request path (API triggers only)
ctx.trigger.method      # HTTP method (API triggers only)
ctx.trigger.expression  # cron expression (cron triggers only)
```

Type guard methods are also available:

```python
if ctx.is_api():
    # Handle API request
elif ctx.is_queue():
    # Handle queue event
elif ctx.is_cron():
    # Handle cron event
```

#### `ctx.get_data()` Helper

The `ctx.get_data()` method normalizes input extraction across trigger types:

- **HTTP triggers:** returns `request.body`
- **Queue triggers:** returns the queue data directly
- **Cron triggers:** returns `None`

#### Builder Pattern (Alternative)

For steps with many triggers, the `MultiTriggerStepBuilder` provides a chainable API:

```python
from motia import multi_trigger_step, http, queue, cron

my_step = (
    multi_trigger_step({
        "name": "MyStep",
        "triggers": [queue("events"), http("POST", "/events"), cron("0 */5 * * * *")],
        "enqueues": ["processed"],
    })
    .on_queue(queue_handler)
    .on_http(http_handler)
    .on_cron(cron_handler)
    .handlers()
)
```

The `StepBuilder` is also available for single-trigger steps:

```python
from motia import step, queue

my_step = step({
    "name": "MyStep",
    "triggers": [queue("events")],
}).handle(my_handler)
```

### Trigger Conditions

Every trigger helper (`http()`, `queue()`, `cron()`, `state()`, `stream()`) accepts an optional `condition` parameter — a function that determines whether the handler should run:

```python
from typing import Any

from motia import FlowContext, queue


def is_high_value(input_data: Any, ctx: FlowContext[Any]) -> bool:
    data = input_data or {}
    return data.get("amount", 0) > 1000


config = {
    "name": "HighValueOrders",
    "triggers": [queue("order.created", condition=is_high_value)],
    "enqueues": ["order.processed"],
}
```

Conditions can also be async and are supported on all trigger types:

```python
http("POST", "/orders/premium", condition=api_premium_check)
cron("0 0 9 * * *", condition=is_business_hours)
state(condition=lambda input, ctx: input.group_id == "users")
stream("todo", condition=lambda input, ctx: input.event.type == "create")
```

### Streams

Stream access changed from `ctx.streams` attribute access to module-level `Stream("name")` declarations.

#### Before (Old)

```python
# Streams accessed directly from the handler context
async def handler(input, ctx):
    # Read from stream via ctx.streams
    move_stream = await ctx.streams.chessGameMove.get(game_id, move_id)

    # Modify and write back
    move_stream["evaluation"] = evaluation
    await ctx.streams.chessGameMove.set(game_id, move_id, move_stream)
```

#### After (New)

```python
from typing import Any

from motia import FlowContext, Stream, queue
from pydantic import BaseModel, Field


# Declare streams at module level (NOT inside the handler)
chess_game_move_stream: Stream[dict[str, Any]] = Stream("chessGameMove")


class EvaluateInput(BaseModel):
    gameId: str = Field(description="The ID of the game")
    moveId: str = Field(description="The ID of the move")
    fenBefore: str = Field(description="FEN before the move")
    fenAfter: str = Field(description="FEN after the move")


config = {
    "name": "EvaluatePlayerMove",
    "flows": ["chess"],
    "triggers": [queue("evaluate-player-move", input=EvaluateInput.model_json_schema())],
    "enqueues": [],
}


async def handler(input_data: dict[str, Any], ctx: FlowContext[Any]) -> None:
    payload = EvaluateInput.model_validate(input_data)

    # Read from stream using module-level Stream object
    move = await chess_game_move_stream.get(payload.gameId, payload.moveId)

    # Modify and write back
    move["evaluation"] = {"score": 0.5}
    await chess_game_move_stream.set(payload.gameId, payload.moveId, move)
```

#### Key Differences

1. Streams are no longer accessed via `ctx.streams.streamName` -- instead, declare a `Stream("name")` at module level.
2. The stream variable is used directly in the handler (not through `ctx`).
3. The stream name in `Stream("chessGameMove")` must match the `name` field in the corresponding `.stream.ts` config.

**Stream operations available in Python:**

```python
stream: Stream[dict] = Stream("myStream")

# Get a single item
item = await stream.get(group_id, item_id)          # returns dict | None

# Set (create or update) an item
await stream.set(group_id, item_id, data)

# List all items in a group (aliases: list() and get_group())
items = await stream.list(group_id)

# Delete an item
await stream.delete(group_id, item_id)

# Atomic update with operations
await stream.update(group_id, item_id, [
    {"op": "set", "path": "/status", "value": "completed"},
    {"op": "increment", "path": "/count", "value": 1},
])

# List all group IDs in the stream
groups = await stream.list_groups()
```

### State

The state API is accessed via `ctx.state` in handlers. The main change is `get_group()` → `list()`:

#### Before (Old)

```python
# Read all items in a group
orders = await context.state.get_group("orders_python")

# Get a single item
order = await context.state.get("orders", order_id)

# Set an item
await context.state.set("orders", order_id, order_data)
```

#### After (New)

```python
# Read all items in a group
orders = await ctx.state.list("orders_python")

# Get a single item (unchanged)
order = await ctx.state.get("orders", order_id)

# Set an item (unchanged)
await ctx.state.set("orders", order_id, order_data)

# Delete an item (unchanged)
await ctx.state.delete("orders", order_id)
```

Additional state operations available in the new SDK:

```python
# Atomic update with operations
await ctx.state.update("orders", order_id, [
    {"op": "set", "path": "/status", "value": "shipped"},
])

# Clear all items in a scope
await ctx.state.clear("orders")

# List all scope IDs
scopes = await ctx.state.list_groups()
```

### Python Imports Reference

All Motia imports come from the `motia` package:

```python
# Trigger helpers
from motia import http, queue, cron, state, stream  # also: api (deprecated alias for http)

# Core types
from motia import ApiRequest, ApiResponse, FlowContext
from motia import Stream, StreamConfig

# Trigger input types (for state and stream trigger handlers)
from motia import StateTriggerInput, StreamTriggerInput, StreamEvent

# Config types
from motia import StepConfig, TriggerInfo, QueryParam

# Step builders (alternative to config dict + handler function)
from motia import step, StepBuilder
from motia import multi_trigger_step, MultiTriggerStepBuilder

# Middleware
from motia import ApiMiddleware
```

### Real-World Migration Example

Here is the actual before/after of a real Python step migration (from the ChessArena project):

**Before** (`evaluate_player_move_step.py`):

```python
import chess
import chess.engine
import os
from pydantic import BaseModel, Field

class EvaluatePlayerMoveInput(BaseModel):
    fenBefore: str = Field(description="The FEN of the game before the move")
    fenAfter: str = Field(description="The FEN of the game after the move")
    gameId: str = Field(description="The ID of the game")
    moveId: str = Field(description="The ID of the move")
    player: str = Field(description="The player who made the move")

config = {
    "type": "event",
    "name": "EvaluatePlayerMove",
    "description": "Evaluates the move picked by a player",
    "subscribes": ["evaluate-player-move"],
    "emits": [],
    "flows": ["chess"],
    "input": EvaluatePlayerMoveInput.model_json_schema(),
    "includeFiles": ["../../lib/stockfish"]
}

async def handler(input: EvaluatePlayerMoveInput, ctx):
    logger = ctx.logger
    fen_before = input.get("fenBefore")
    game_id = input.get("gameId")
    move_id = input.get("moveId")

    # ... (business logic omitted for brevity) ...

    # Streams accessed via ctx.streams
    move_stream = await ctx.streams.chessGameMove.get(game_id, move_id)
    move_stream["evaluation"] = evaluation
    await ctx.streams.chessGameMove.set(game_id, move_id, move_stream)
```

**After** (`evaluate_player_move_step.py`):

```python
import os
from typing import Any, Literal

import chess
import chess.engine
from motia import FlowContext, Stream, queue
from pydantic import BaseModel, Field

# Stream declared at module level
chess_game_move_stream: Stream[dict[str, Any]] = Stream("chessGameMove")


class EvaluatePlayerMoveInput(BaseModel):
    fenBefore: str = Field(description="The FEN of the game before the move")
    fenAfter: str = Field(description="The FEN of the game after the move")
    gameId: str = Field(description="The ID of the game")
    moveId: str = Field(description="The ID of the move")
    player: Literal["white", "black"] = Field(description="The player who made the move")


config = {
    "name": "EvaluatePlayerMove",
    "description": "Evaluates the move picked by a player",
    "flows": ["chess"],
    "triggers": [queue("evaluate-player-move", input=EvaluatePlayerMoveInput.model_json_schema())],
    "enqueues": [],
    "includeFiles": ["../../lib/stockfish"],
}


async def handler(input_data: dict[str, Any], ctx: FlowContext[Any]) -> None:
    logger = ctx.logger
    payload = EvaluatePlayerMoveInput.model_validate(input_data)

    # ... (business logic omitted for brevity) ...

    # Stream accessed via module-level Stream object
    move_stream = await chess_game_move_stream.get(payload.gameId, payload.moveId)
    move_stream["evaluation"] = evaluation
    await chess_game_move_stream.set(payload.gameId, payload.moveId, move_stream)
```

#### What Changed
1. File naming `*_step.py` is unchanged.
2. Added imports: `from motia import FlowContext, Stream, queue`
3. Config: removed `"type": "event"`, `"subscribes"`, `"emits"` -- replaced with `triggers: [queue(...)]` and `"enqueues"`
4. Handler: `async def handler(input, ctx)` → `async def handler(input_data: dict[str, Any], ctx: FlowContext[Any]) -> None`
5. Input: `input.get("fenBefore")` → `payload = EvaluatePlayerMoveInput.model_validate(input_data)` then `payload.fenBefore`
6. Streams: `ctx.streams.chessGameMove.get(...)` → module-level `Stream("chessGameMove")` then `chess_game_move_stream.get(...)`
7. Dependencies: `requirements.txt` deleted, replaced by `pyproject.toml` with `motia[otel]` and `iii-sdk`

### Python Migration Checklist

#### Project Setup
- [ ] Create `pyproject.toml` with `motia[otel]`, `iii-sdk`, and `pydantic` dependencies
- [ ] Run `uv sync` to install dependencies — Node.js is no longer required
- [ ] Add a Python ExecModule entry in `config.yaml`
- [ ] Delete `motia.config.ts`, `package.json`, and `tsconfig.json` if this is a Python-only project (replaced by `config.yaml` and `pyproject.toml`)
- [ ] Delete `motia-workbench.json` (replaced by iii Console — see [Section 13](#13-workbench-plugins-and-console))

#### File Changes
- [ ] Rename step files from `*_step.py` to `*_step.py`
- [ ] Delete `requirements.txt` if present (replaced by `pyproject.toml`)

#### Config Migration
- [ ] Remove `"type"` field from all step configs
- [ ] Replace `"subscribes": [...]` with `"triggers": [queue(...)]`
- [ ] Replace `"method"` / `"path"` at config root with `"triggers": [http(...)]`
- [ ] Replace `"cron": "..."` at config root with `"triggers": [cron("...")]`
- [ ] Convert 5-field cron expressions to 6-field (prepend seconds; 7th year field optional)
- [ ] Move `"input": schema` from config root into `queue("topic", input=schema)`
- [ ] Rename `"emits"` to `"enqueues"` in all configs
- [ ] Move `"bodySchema"` from config root into `http("POST", "/path", body_schema=...)` trigger
- [ ] Move `"responseSchema"` from config root into `http("POST", "/path", response_schema=...)` trigger
- [ ] Move `"queryParams"` from config root into `http("GET", "/path", query_params=...)` trigger

#### Handler Migration
- [ ] Replace `context.emit()` with `ctx.enqueue()` in all handlers
- [ ] Replace `context.state.get_group()` with `ctx.state.list()`
- [ ] Update cron handler signature from `async def handler(context)` to `async def handler(input_data, ctx)`
- [ ] Update API handlers to use `ApiRequest` / `ApiResponse` types
- [ ] Replace `req.get("pathParams", {}).get("x")` with `request.path_params["x"]`
- [ ] Replace `req.get("queryParams", {})` with `request.query_params`
- [ ] Replace `req.get("headers", {})` with `request.headers`
- [ ] Replace `return {"status": N, "body": {...}}` with `return ApiResponse(status=N, body={...})`
- [ ] Replace `ctx.streams.streamName.get(...)` with module-level `Stream("name")` declarations
- [ ] Replace attribute access on handler input (`args.field`) with dict access (`input_data["field"]` or `input_data.get("field")`)

#### New Features (Optional)
- [ ] Migrate state-triggered steps to use `state()` trigger and `StateTriggerInput`
- [ ] Migrate stream event listeners to use `stream()` trigger and `StreamTriggerInput`
- [ ] Use `ctx.match()` for steps with mixed trigger types
- [ ] Add `condition` functions to triggers where input filtering is needed
- [ ] Use `stream.update()` for atomic operations where appropriate

---

## 13. Workbench, Plugins, and Console

### Workbench Replaced by iii Console

The Motia Workbench (the local visual flow editor, configured via `motia-workbench.json`) has been replaced by the **iii Console**. The console provides a richer experience for visualizing and managing your flows, traces, and infrastructure.

> Refer to the [iii quickstart documentation](https://iii.dev/docs/quickstart) for iii Console installation instructions.

### Workbench Plugins Sunset

Workbench plugins (custom UI panels and extensions rendered inside the Workbench) have been **sunset** and are no longer supported. If your project relied on workbench plugins, you will need to find alternative approaches for any custom UI functionality they provided.

- Delete any `.ui.step.ts` or noop step files that were used exclusively for workbench rendering.
- Remove any React/JSX workbench plugin code that is no longer needed.

---

## 14. OpenAPI Generation

Motia's automatic OpenAPI/Swagger spec generation from HTTP step schemas is currently a **work in progress**. This feature is not yet available in the new version. If your project relied on generated OpenAPI specs, be aware that this capability will be restored in a future release.
