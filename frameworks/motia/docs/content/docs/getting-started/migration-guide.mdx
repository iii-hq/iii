---
title: 0.17 to 1.0 Migration Guide
description: Migrating from Motia v0.17.x to the Motia 1.0-RC Framework powered by iii.
---

This guide covers migrating from **Motia v0.17.x** to the **Motia 1.0-RC** framework. It is organized by area of concern so you can migrate incrementally.

<Callout title="iii is required" type="warn">
  Motia now requires the **iii engine** to run. Install iii from [iii.dev](https://iii.dev) before proceeding with the migration. All adapter and infrastructure configuration is now done through iii via a `config.yaml` file -- the SDK itself no longer handles any of this.
</Callout>

<Callout title="MD Migration Guide" type="warn">
  A Markdown version of this guide is available at [motia/MIGRATION_GUIDE.md](https://github.com/MotiaDev/motia/blob/main/MIGRATION_GUIDE.md).
</Callout>

## Configuration

### Project Config

The old `motia.config.ts` (using `defineConfig`) is replaced by two files managed by iii:

| Concern | Old | New |
|---|---|---|
| Project config & plugins | `motia.config.ts` (`defineConfig({...})`) | Removed (handled by iii engine via `config.yaml`) |
| Module/adapter config | N/A | `config.yaml` (iii engine config) |
| Auth & hooks | `streamAuth` in `motia.config.ts` | `motia.config.ts` (simplified, exports only auth hooks) |
| Build externals | `.esbuildrc.json` | Removed |
| Workbench UI layout | `motia-workbench.json` | Removed (see [Workbench, Plugins, and Console](#workbench-plugins-and-console)) |

<Tabs items={['Old (motia.config.ts)', 'New (config.yaml)', 'New (motia.config.ts)']}>
<Tab value="Old (motia.config.ts)">

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

</Tab>
<Tab value="New (config.yaml)">

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
      exec:                           # Choose ONE based on your runtime
        - npx motia dev               # Node.js (default)
        # - bun run --enable-source-maps dist/index-dev.js  # Bun (alternative)
```

</Tab>
<Tab value="New (motia.config.ts)">

```typescript
import type { AuthenticateStream } from 'motia'
import { logger } from 'motia'

export const authenticateStream: AuthenticateStream = async (req) => {
  logger.info('Authenticating stream', { req })
  return { context: { userId: 'sergio' } }
}
```

</Tab>
</Tabs>

### Dev Command

| Old | New |
|---|---|
| `motia dev` | `iii` |
| `motia build` | `motia build` (unchanged) |

### Files to Delete

- `motia-workbench.json`
- `.motia/` directory — **warning:** this will delete any local stream and state data persisted by the old engine; back up first if needed

<Callout type="info">
  `motia.config.ts` is **not deleted** -- it is simplified. Remove the `defineConfig` wrapper, all plugin imports, and the `plugins` array. Keep only the authentication hook exports (see the "New" tab above).
</Callout>

---

## Module System and Runtime

The new Motia **does not enforce a specific module system or runtime**. You are free to use CommonJS, ESM, Node.js, Bun, or any compatible runtime. The framework adapts to your project's setup.

### Runtime Support

Motia now has first-class support for **Bun** in addition to Node.js. You can choose whichever runtime fits your project:

| Runtime | Dev Command Example | Production Example |
|---|---|---|
| Node.js | `npx motia dev` | `node dist/index-production.js` |
| Bun | `bun run dist/index-dev.js` | `bun run --enable-source-maps dist/index-production.js` |

### Module System

You can use either CommonJS or ESM -- the choice is yours. If you want to adopt ESM (recommended for Bun compatibility and modern tooling), update your project:

```json title="package.json (optional)"
{
  "type": "module"
}
```

```jsonc title="tsconfig.json (optional)"
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

## Steps and Triggers

<Callout title="No more step types" type="info">
  This is the most important conceptual change in new Motia: **there are no longer separate "step types"**. In the old version, you had API steps, Event steps, and Cron steps -- each with its own config type. In the new version, **everything is just a Step**. What used to determine the "type" of a step is now expressed through its **triggers** -- an array of trigger definitions that describe how and when the step is activated. A single step can have multiple triggers of different kinds (HTTP, queue, cron, state, stream).
</Callout>

### Config Type Changes

| Old | New |
|---|---|
| `ApiRouteConfig` | `StepConfig` |
| `EventConfig` | `StepConfig` |
| `CronConfig` | `StepConfig` |
| `type: 'api' \| 'event' \| 'cron'` | `triggers: [{ type: 'http' \| 'queue' \| 'cron' \| 'state' \| 'stream' }]` |
| `emits: ['topic']` | `enqueues: ['topic']` |
| `subscribes: ['topic']` | Moved into trigger: `{ type: 'queue', topic: '...' }` |

### Handler Type Changes

| Old | New |
|---|---|
| `Handlers['StepName']` | `Handlers<typeof config>` |
| `ctx.emit({ topic, data })` | `import { enqueue } from 'motia'` then `enqueue({ topic, data })` |
| `ctx.logger` | `import { logger } from 'motia'` |
| `ctx.state` | `import { stateManager } from 'motia'` |
| `ctx.streams.name` | `import { myStream } from './my.stream'` |

### Type Safety

The new version uses `as const satisfies StepConfig` for full type inference:

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
export const config: ApiRouteConfig = {
  type: 'api',
  name: 'MyStep',
  // ...
}
export const handler: Handlers['MyStep'] = async (req, ctx) => { /* ... */ }
```

</Tab>
<Tab value="New">

```typescript
export const config = {
  name: 'MyStep',
  // ...
  triggers: [{ type: 'http', method: 'GET', path: '/my-step' }],
  enqueues: [],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }, ctx) => { /* ... */ }
```

</Tab>
</Tabs>

---

## HTTP Triggers

In the old version these were "API steps" -- a dedicated step type with `type: 'api'`. In the new version, HTTP is just a **trigger type** (`type: 'http'`) on a regular step.

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
import { ApiRouteConfig, Handlers } from 'motia'
import { z } from 'zod'

const bodySchema = z.object({ name: z.string(), email: z.string() })

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
  await emit({ topic: 'user-created', data: { name, email } })
  return { status: 200, body: { id: 'user-123' } }
}
```

</Tab>
<Tab value="New">

```typescript
import { enqueue, type Handlers, logger, type StepConfig } from 'motia'
import { z } from 'zod'

const bodySchema = z.object({ name: z.string(), email: z.string() })

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
      middleware: [coreMiddleware, validateBearerToken],
    },
  ],
  enqueues: ['user-created'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  const { name, email } = request.body
  logger.info('Creating user', { name, email })
  await enqueue({ topic: 'user-created', data: { name, email } })
  return { status: 200, body: { id: 'user-123' } }
}
```

</Tab>
</Tabs>

**Key differences:**

1. `type: 'api'` is now `type: 'http'` inside a trigger object.
2. `method`, `path`, `bodySchema`, `responseSchema`, `middleware` all move inside the trigger.
3. `emits` becomes `enqueues` at the config level.
4. `emit()` becomes `enqueue()` -- now imported from `'motia'` instead of accessed via `ctx`.
5. `logger` is imported from `'motia'` instead of destructured from `ctx`.
6. Config type changes from `ApiRouteConfig` to `StepConfig` with `as const satisfies`.

### HTTP Helper Shorthand

```typescript
import { http } from 'motia'

export const config = {
  name: 'CreateTodo',
  flows: ['todo-app'],
  triggers: [
    http('POST', '/todo', {
      bodySchema: z.object({ description: z.string() }),
      responseSchema: { 200: todoSchema, 400: z.object({ error: z.string() }) },
    }),
  ],
  enqueues: [],
} as const satisfies StepConfig
```

---

## Queue Triggers (formerly Event Steps)

The concept of "event steps" that subscribe to topics no longer exists as a step type. Instead, subscribing to a topic is now a **queue trigger** on a regular step.

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
import { EventConfig, Handlers } from 'motia'
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
  await emit({ topic: 'deploy-version-v2', data: { deploymentId: data.deploymentId } })
}
```

</Tab>
<Tab value="New">

```typescript
import { enqueue, type Handlers, logger, type StepConfig } from 'motia'
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

export const handler: Handlers<typeof config> = async (input) => {
  logger.info('Deploying environment', { deploymentId: input.deploymentId })
  await enqueue({ topic: 'deploy-version-v2', data: { deploymentId: input.deploymentId } })
}
```

</Tab>
</Tabs>

| Old | New |
|---|---|
| `type: 'event'` | `triggers: [{ type: 'queue', topic, input }]` |
| `subscribes: ['topic']` | `topic` field inside trigger |
| `emits: ['topic']` | `enqueues: ['topic']` |
| `input: schema` | `input: schema` inside trigger (or wrap with `jsonSchema()`) |
| `infrastructure: {...}` at config root | `config: {...}` inside the queue trigger |
| `emit({ topic, data })` | `enqueue({ topic, data })` |

### Using `jsonSchema()` Wrapper

When the input schema needs JSON schema conversion for the engine, use the `jsonSchema()` wrapper:

```typescript
import { jsonSchema } from 'motia'

triggers: [
  {
    type: 'queue',
    topic: 'notification',
    input: jsonSchema(
      z.object({ email: z.string(), templateId: z.string() })
    ),
  },
]
```

---

## Cron Triggers

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
import { CronConfig, Handlers } from 'motia'

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
  await emit({ topic: 'collect-metrics', data: { targetDate: new Date().toISOString() } })
}
```

</Tab>
<Tab value="New">

```typescript
import { enqueue, type Handlers, logger, type StepConfig } from 'motia'

export const config = {
  name: 'DailyMetricsCollection',
  description: 'Collects metrics daily at midnight',
  flows: ['metrics-collection-flow'],
  triggers: [{ type: 'cron', expression: '0 0 5 * * * *' }],
  enqueues: ['collect-metrics'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async () => {
  logger.info('Collecting metrics')
  await enqueue({ topic: 'collect-metrics', data: { targetDate: new Date().toISOString() } })
}
```

</Tab>
</Tabs>

| Old | New |
|---|---|
| `type: 'cron'` at config root | `triggers: [{ type: 'cron', expression }]` |
| `cron: '0 5 * * *'` (5-field) | `expression: '0 0 5 * * * *'` (7-field, includes seconds and year) |
| Handler: `async ({ logger, emit })` | Handler: `async ()` with `logger` and `enqueue` imported from `'motia'` |
| `emit()` | `enqueue()` |

### Cron Expression Format

The new engine uses a 7-field cron expression:

```text
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

| Old (5-field) | New (7-field) | Meaning |
|---|---|---|
| `0 5 * * *` | `0 0 5 * * * *` | Daily at 5:00 AM |
| `0 2 * * *` | `0 0 2 * * * *` | Daily at 2:00 AM |
| `*/5 * * * *` | `0 */5 * * * * *` | Every 5 minutes |
| `0 0 * * 0` | `0 0 0 * * 0 *` | Weekly on Sunday at midnight |

---

## Streams

Stream definitions remain similar but the access API has changed.

### Stream Config

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
import { StreamConfig } from 'motia'
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

</Tab>
<Tab value="New">

```typescript
import { logger, Stream, type StreamConfig } from 'motia'
import { z } from 'zod'

export const config: StreamConfig = {
  name: 'deployment',
  baseConfig: { storageType: 'default' },
  schema: z.object({
    id: z.string(),
    status: z.enum(['pending', 'progress', 'completed', 'failed']),
    message: z.string().optional(),
  }),
  onJoin: async (subscription, _context, authContext) => {
    logger.info('Client joined stream', { subscription, authContext })
    return { unauthorized: false }
  },
  onLeave: async (subscription, _context, authContext) => {
    logger.info('Client left stream', { subscription, authContext })
  },
}

export const deploymentStream = new Stream(config)
```

</Tab>
</Tabs>

### Stream Operations API

Streams are no longer accessed via `ctx.streams`. Instead, create a `Stream` instance in a `.stream.ts` file and import it into your steps:

| Operation | Old | New |
|---|---|---|
| Get | `ctx.streams.name.get(id, key)` | `myStream.get(groupId, id)` |
| Set | `ctx.streams.name.set(id, key, value)` | `myStream.set(groupId, id, value)` |
| Update | N/A | `myStream.update(groupId, id, UpdateOp[])` |
| Delete | `ctx.streams.name.delete(id, key)` | `myStream.delete(groupId, id)` |

The parameter naming changed from `(id, key)` to `(groupId, id)` to better reflect the data model: a stream is partitioned by groups, and within each group items are identified by id.

### Atomic Updates with `UpdateOp`

```typescript
import type { UpdateOp } from 'motia'
import { deploymentStream } from './deployment.stream'

await deploymentStream.update('merge-groups', traceId, [
  { type: 'increment', path: 'completedSteps', by: 1 },
  { type: 'set', path: 'status', value: 'progress' },
  { type: 'decrement', path: 'retries', by: 1 },
])
```

| Type | Fields | Description |
|---|---|---|
| `set` | `path`, `value` | Set a field to a value (overwrite) |
| `merge` | `path` (optional), `value` | Merge an object into the existing value (object-only) |
| `increment` | `path`, `by` | Increment a numeric field |
| `decrement` | `path`, `by` | Decrement a numeric field |
| `remove` | `path` | Remove a field entirely |

### Migration Example

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
const streamData = await streams.deployment.get(deploymentId, 'data')
streamData.status = 'completed'
streamData.message = 'Done'
await streams.deployment.set(deploymentId, 'data', streamData)
```

</Tab>
<Tab value="New">

```typescript
import { deploymentStream } from './deployment.stream'

await deploymentStream.update('data', deploymentId, [
  { type: 'set', path: 'status', value: 'completed' },
  { type: 'set', path: 'message', value: 'Done' },
])
```

</Tab>
</Tabs>

### Stream Triggers

Steps can now react to stream changes. The handler receives a `StreamWrapperMessage`:

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

## State

State provides key-value storage grouped by a namespace. The core `get`, `set`, and `list` operations remain the same. State is now accessed via `stateManager` imported from `'motia'` instead of `ctx.state`. The new version also introduces **atomic updates** via the `update` method, and **state triggers**.

### API (now via `stateManager` import)

```typescript
import { stateManager } from 'motia'

await stateManager.set('orders', orderId, orderData)
const order = await stateManager.get<Order>('orders', orderId)
const allOrders = await stateManager.list<Order>('orders')
```

### Atomic Updates with `update()`

<Callout title="New feature" type="info">
  The `update()` method eliminates race conditions from manual get-then-set patterns by performing atomic operations.
</Callout>

```typescript
import { stateManager } from 'motia'

await stateManager.update<Order>('orders', orderId, [
  { type: 'increment', path: 'completedSteps', by: 1 },
  { type: 'set', path: 'status', value: 'shipped' },
  { type: 'decrement', path: 'retries', by: 1 },
])
```

Uses the same `UpdateOp` interface as streams. See [Streams](#atomic-updates-with-updateop) for the full list of operations.

### State Triggers

<Callout title="Brand new feature" type="info">
  State triggers enable powerful reactive patterns -- for example, triggering a step when a parallel merge completes, without polling or manual coordination.
</Callout>

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

The handler receives the state change event as its first argument, including `new_value`, `old_value`, `item_id`, and `group_id`.

---

## Middleware

### Old Approach

```typescript
import { ApiMiddleware } from 'motia'

export const validateBearerToken: ApiMiddleware = async (req, ctx, next) => {
  const authToken = req.headers['authorization'] as string
  if (!authToken) {
    return { status: 401, body: { error: 'Unauthorized' } }
  }
  const [, token] = authToken.split(' ')
  const decoded = jwt.verify(token, process.env.JWT_SECRET!) as TokenData
  req.tokenInfo = decoded
  return next()
}

// In step config (root level):
export const config: ApiRouteConfig = {
  type: 'api',
  name: 'GetUser',
  middleware: [coreMiddleware, validateBearerToken],
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

```typescript title="Alternative: handler-level auth"
export async function requireAuth(request: MotiaHttpRequest<any>): Promise<TokenData> {
  const authToken = request.headers['authorization'] as string
  if (!authToken) {
    throw new HttpError(401, 'Unauthorized')
  }
  const [, token] = authToken.split(' ')
  return jwt.verify(token, process.env.JWT_SECRET!) as TokenData
}

export const handler: Handlers<typeof config> = async ({ request }) => {
  const tokenData = await requireAuth(request)
  // ... rest of handler
}
```

---

## New Features

### Multi-Trigger Steps

A single step can now respond to multiple trigger types:

```typescript
export const config = {
  name: 'ProcessOrder',
  flows: ['orders'],
  triggers: [
    { type: 'queue', topic: 'order.created', input: orderSchema },
    { type: 'http', method: 'POST', path: '/orders/manual', bodySchema: orderSchema },
    { type: 'cron', expression: '0 * * * * * *' },
  ],
  enqueues: ['order.processed'],
} as const satisfies StepConfig
```

### The `step()` Helper

For multi-trigger steps, the `step()` helper provides `ctx.getData()` and `ctx.match()`:

```typescript
import { http, logger, queue, step } from 'motia'

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
  const data = ctx.getData()

  return ctx.match({
    http: async ({ request }) => {
      return { status: 200, body: { success: true } }
    },
    queue: async (queueInput) => {
      logger.info('Processing from queue', { queueInput })
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
      return input.amount > 1000
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

## Migration Checklist

### Project Setup

- Install the iii engine from [iii.dev](https://iii.dev)
- Create `config.yaml` with module definitions (stream, state, api, queue, cron, exec)
- Create `motia.config.ts` for authentication hooks (if needed)
- Simplify `motia.config.ts`: remove `defineConfig`, all plugin imports, and the `plugins` array; keep only auth hook exports
- Delete `motia-workbench.json`
- Delete `.motia/` directory (**warning:** this will delete any local stream and state data persisted by the old engine; back up first if needed)
- Update dev script from `motia dev` to `iii`
- Choose your runtime (Node.js or Bun) and module system (CommonJS or ESM)

### Steps

- Replace all `ApiRouteConfig` / `EventConfig` / `CronConfig` imports with `StepConfig`
- Convert all step configs to use `triggers[]` and `enqueues[]`
- Add `as const satisfies StepConfig` to all configs
- Replace `Handlers['StepName']` with `Handlers<typeof config>`
- Rename all `emit()` calls to `enqueue()`
- Rename all `emits` config fields to `enqueues`
- Move `subscribes` into queue triggers
- Move `method`, `path`, `bodySchema`, `responseSchema`, `middleware` into HTTP triggers
- Move `infrastructure` from config root into queue triggers as `config`
- Change `type: 'api'` to `type: 'http'` in all triggers
- Move `cron` into cron triggers as `expression` (and convert to 7-field format)
- Remove `type` field from config root
- Remove `middleware` field from all step configs
- Replace `virtualEmits` with `virtualEnqueues` (format changes from `[{ topic, label }]` to `['topic']`)

### Context API

- Replace `ctx.logger` / `context.logger` with `import { logger } from 'motia'`
- Replace `ctx.enqueue` / `context.enqueue` with `import { enqueue } from 'motia'`
- Replace `ctx.state` / `context.state` with `import { stateManager } from 'motia'`
- Replace `ctx.streams.name` / `context.streams.name` with stream instances imported from `.stream.ts` files
- Create `.stream.ts` files with `new Stream(config)` for each stream
- Remove `state`, `enqueue`, `logger`, `streams` from handler destructuring of `ctx`
- If `ctx` was only used for those removed properties, omit the second handler argument entirely

### Streams

- Create `.stream.ts` files with `new Stream(config)` and import instances into steps
- Update stream access calls: `get(id, key)` to `myStream.get(groupId, id)`
- Update stream access calls: `set(id, key, value)` to `myStream.set(groupId, id, value)`
- Replace read-modify-write patterns with `myStream.update(groupId, id, UpdateOp[])` where possible
- Add `onJoin` / `onLeave` hooks to stream configs if real-time subscription auth is needed

### State

- Replace `ctx.state` with `import { stateManager } from 'motia'`
- Adopt `stateManager.update()` with `UpdateOp[]` to replace manual get-then-set patterns
- Consider using state triggers for reactive workflows

### Middleware

- Move `middleware` arrays from config root into the corresponding HTTP trigger objects
- Alternatively, extract authentication logic into shared utility functions called in handlers

### Cron Expressions

- Convert all 5-field cron expressions to 7-field format (prepend seconds, append year)
- Rename `cron` field to `expression` inside trigger objects

### Python (if applicable)

- Install `motia` as a standalone Python package (npm/Node.js no longer required)
- Add a separate ExecModule entry in `config.yaml` for the Python runtime
- Refer to the dedicated Python migration guide for step-level changes

### Workbench and Plugins

- Delete `motia-workbench.json`
- Remove any `.ui.step.ts` or noop step files used exclusively for workbench rendering
- Remove any workbench plugin code (React/JSX components for workbench panels)
- Familiarize with the iii Console as the replacement for the Workbench

---

## Python Runtime

<Callout title="Major change for Python developers" type="warn">
  In the old Motia, Python steps were managed by the Node.js runtime. Python developers **previously needed Node.js and npm installed**. This is no longer the case. Python is now a fully independent runtime.
</Callout>

In the new Motia, **runtimes are fully independent**. There is a dedicated **Motia Python** SDK (`motia-py`) that runs as its own standalone process, communicating directly with the iii engine. **Python developers no longer need Node.js, npm, or any JavaScript tooling whatsoever.**

| Aspect | Old | New |
|---|---|---|
| Python execution | Spawned as child process by Node runtime | Independent process managed by iii engine |
| Node.js required for Python? | Yes | **No** |
| SDK | Single `motia` npm package handled both | Separate `motia-py` (Python) and `motia` (Node) packages |
| Configuration | Shared with Node steps | Own `config.yaml` ExecModule entry |

### For Mixed Projects (Node + Python)

Configure **separate ExecModule entries** in `config.yaml`:

```yaml
modules:
  - class: modules::shell::ExecModule
    config:
      watch:                          # Glob patterns to watch for hot-reload
        - steps/**/*.ts
        - motia.config.ts
      exec:                           # Choose ONE based on your runtime
        - npx motia dev               # Node.js (default)
        # - bun run --enable-source-maps dist/index-dev.js  # Bun (alternative)

  - class: modules::shell::ExecModule
    config:
      watch:                          # Glob patterns to watch for hot-reload
        - steps/**/*.py
      exec:
        - uv run motia dev --dir steps
```

<Callout type="info">
  A dedicated migration guide for Python projects and steps will be provided in a separate document. This guide focuses on the Node.js/TypeScript migration path.
</Callout>

---

## Workbench, Plugins, and Console

### Workbench Replaced by iii Console

The Motia Workbench (the local visual flow editor, configured via `motia-workbench.json`) has been replaced by the **iii Console**. The console provides a richer experience for visualizing and managing your flows, traces, and infrastructure.

<Callout type="info">
  Refer to the [iii quickstart documentation](https://iii.dev/docs/tutorials/quickstart) for iii Console installation instructions.
</Callout>

### Workbench Plugins Sunset

Workbench plugins (custom UI panels and extensions rendered inside the Workbench) have been **sunset** and are no longer supported. If your project relied on workbench plugins, you will need to find alternative approaches for any custom UI functionality they provided.

- Delete any `.ui.step.ts` or noop step files that were used exclusively for workbench rendering.
- Remove any React/JSX workbench plugin code that is no longer needed.

---

## OpenAPI Generation

<Callout type="info">
  OpenAPI spec generation from HTTP step schemas is planned but **not yet available** in the new Motia version. This section will be updated once the feature is released.
</Callout>

