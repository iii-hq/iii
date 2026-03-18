---
title: API Reference
description: Complete API reference for the Motia framework
---

Everything you need to know about Motia's APIs. This reference covers all the types, methods, and configurations available when building with Motia.

If you're new to Motia, start with the [Steps guide](/docs/concepts/steps) to understand the basics.

<Callout type="info" title="Package exports">
  For a concise list of npm entrypoints and exports, see [motia Package Exports](/docs/development-guide/motia-package-exports).
</Callout>

## Step Configuration

Every Step needs a config. The unified `StepConfig` type works for all step types -- the `triggers` array determines what activates the step.

### StepConfig

```typescript
type StepConfig = {
  name: string
  description?: string
  triggers: readonly TriggerConfig[]
  enqueues?: readonly Enqueue[]
  virtualEnqueues?: readonly Enqueue[]
  virtualSubscribes?: readonly string[]
  flows?: readonly string[]
  includeFiles?: readonly string[]
}
```

**Required fields:**
- `name` - Unique identifier for this Step
- `triggers` - Array of triggers that activate this step (HTTP, queue, cron, state, stream)

**Optional fields:**
- `description` - Human-readable description
- `enqueues` - Topics this Step can enqueue
- `virtualEnqueues` - Topics shown in the iii development console but not actually enqueued (gray connections)
- `virtualSubscribes` - Topics shown in the iii development console for flow visualization
- `flows` - Flow names for iii development console grouping
- `includeFiles` - Files to bundle with this Step (supports glob patterns, relative to Step file)

---

### TriggerConfig

Triggers define how a step gets activated. A step can have multiple triggers.

```typescript
type TriggerConfig = QueueTrigger | HttpTrigger | CronTrigger | StateTrigger | StreamTrigger
```

#### HttpTrigger

Use this for HTTP endpoints.

```typescript
type HttpTrigger = {
  type: 'http'
  path: string
  method: HttpRouteMethod
  bodySchema?: StepSchemaInput
  responseSchema?: Record<number, StepSchemaInput>
  queryParams?: readonly QueryParam[]
  middleware?: readonly HttpMiddleware[]
  condition?: TriggerCondition
}
```

#### QueueTrigger

Use this for background jobs and event-driven tasks.

```typescript
type QueueTrigger = {
  type: 'queue'
  topic: string
  input?: StepSchemaInput
  condition?: TriggerCondition
  config?: Partial<QueueConfig>
}
```

#### CronTrigger

Use this for scheduled tasks.

```typescript
type CronTrigger = {
  type: 'cron'
  expression: string
  condition?: TriggerCondition
}
```

Use [crontab.guru](https://crontab.guru) to build cron expressions.

#### StateTrigger

Use this to trigger steps based on state changes.

```typescript
type StateTrigger = {
  type: 'state'
  condition?: TriggerCondition
}
```

#### StreamTrigger

Use this to trigger steps from stream events.

```typescript
type StreamTrigger = {
  type: 'stream'
  streamName: string
  groupId?: string
  itemId?: string
  condition?: TriggerCondition
}
```

---

### Trigger Helper Functions

Use these helpers for concise trigger definitions:

```typescript
import { http, queue, cron, state, stream } from 'motia'

http(method: HttpRouteMethod, path: string, options?: HttpOptions, condition?: TriggerCondition): HttpTrigger
queue(topic: string, options?: { input?: StepSchemaInput; config?: Partial<QueueConfig> }, condition?: TriggerCondition): QueueTrigger
cron(expression: string, condition?: TriggerCondition): CronTrigger
state(condition?: TriggerCondition): StateTrigger
stream(streamName: string, condition?: TriggerCondition): StreamTrigger
```

---

### Enqueue Type

```typescript
type Enqueue = string | { topic: string; label?: string; conditional?: boolean }
type EnqueueData<T> = { topic: string; data: T; messageGroupId?: string }
```

---

### Config Examples

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
import { StepConfig, Handlers, http, queue, cron } from 'motia'

export const config = {
  name: 'CreateUser',
  description: 'Creates a new user',
  triggers: [
    http('POST', '/users', {
      bodySchema: z.object({ name: z.string() }),
      responseSchema: {
        201: z.object({ id: z.string(), name: z.string() })
      },
      middleware: [authMiddleware],
      queryParams: [{ name: 'invite', description: 'Invite code' }],
    }),
  ],
  enqueues: ['user.created'],
  virtualEnqueues: ['notification.sent'],
  virtualSubscribes: ['user.invited'],
  flows: ['user-management'],
  includeFiles: ['../../assets/template.html'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='JavaScript'>

```javascript
export const config = {
  name: 'CreateUser',
  description: 'Creates a new user',
  triggers: [
    {
      type: 'http',
      method: 'POST',
      path: '/users',
      bodySchema: z.object({ name: z.string() }),
      responseSchema: {
        201: z.object({ id: z.string(), name: z.string() })
      },
      middleware: [authMiddleware],
      queryParams: [{ name: 'invite', description: 'Invite code' }],
    },
  ],
  enqueues: ['user.created'],
  virtualEnqueues: ['notification.sent'],
  virtualSubscribes: ['user.invited'],
  flows: ['user-management'],
  includeFiles: ['../../assets/template.html'],
}
```

</Tab>
<Tab value='Python'>

```python
from pydantic import BaseModel

class UserResponse(BaseModel):
    id: str
    name: str

config = {
    "name": "CreateUser",
    "description": "Creates a new user",
    "triggers": [
        {
            "type": "http",
            "method": "POST",
            "path": "/users",
            "bodySchema": {"type": "object", "properties": {"name": {"type": "string"}}},
            "responseSchema": {201: UserResponse.model_json_schema()},
            "middleware": [auth_middleware],
            "queryParams": [{"name": "invite", "description": "Invite code"}],
        },
    ],
    "enqueues": ["user.created"],
    "virtualEnqueues": ["notification.sent"],
    "virtualSubscribes": ["user.invited"],
    "flows": ["user-management"],
    "includeFiles": ["../../assets/template.html"],
}
```

</Tab>
</Tabs>

---

### Queue Step Config Example

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
import { StepConfig, queue } from 'motia'

export const config = {
  name: 'ProcessOrder',
  description: 'Processes new orders',
  triggers: [
    queue('order.created', {
      input: z.object({ orderId: z.string(), amount: z.number() }),
      config: { type: 'fifo', maxRetries: 3, visibilityTimeout: 90 },
    }),
  ],
  enqueues: ['order.processed'],
  virtualEnqueues: ['payment.initiated'],
  virtualSubscribes: ['order.cancelled'],
  flows: ['orders'],
  includeFiles: ['./templates/*.html'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='JavaScript'>

```javascript
export const config = {
  name: 'ProcessOrder',
  description: 'Processes new orders',
  triggers: [
    {
      type: 'queue',
      topic: 'order.created',
      input: z.object({ orderId: z.string(), amount: z.number() }),
      config: { type: 'fifo', maxRetries: 3, visibilityTimeout: 90 },
    },
  ],
  enqueues: ['order.processed'],
  virtualEnqueues: ['payment.initiated'],
  virtualSubscribes: ['order.cancelled'],
  flows: ['orders'],
  includeFiles: ['./templates/*.html'],
}
```

</Tab>
<Tab value='Python'>

```python
from pydantic import BaseModel

class OrderInput(BaseModel):
    order_id: str
    amount: float

config = {
    "name": "ProcessOrder",
    "description": "Processes new orders",
    "triggers": [
        {
            "type": "queue",
            "topic": "order.created",
            "input": OrderInput.model_json_schema(),
            "config": {"type": "fifo", "maxRetries": 3, "visibilityTimeout": 90},
        },
    ],
    "enqueues": ["order.processed"],
    "virtualEnqueues": ["payment.initiated"],
    "virtualSubscribes": ["order.cancelled"],
    "flows": ["orders"],
    "includeFiles": ["./templates/*.html"],
}
```

</Tab>
</Tabs>

**Queue config options:**
- `type` - `'fifo'` or `'standard'` (default: `'standard'`)
- `maxRetries` - Max retry attempts (default: 3)
- `visibilityTimeout` - Seconds before message becomes visible again (default: 900)
- `delaySeconds` - Delay before message becomes visible, 0-900 (default: 0)
- `concurrency` - Max parallel message processing per topic
- `backoffType` - Retry backoff strategy (e.g., `'exponential'`)
- `backoffDelayMs` - Base delay in ms for retry backoff

---

### Cron Step Config Example

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
import { StepConfig, cron } from 'motia'

export const config = {
  name: 'DailyReport',
  description: 'Generates daily reports at 9 AM',
  triggers: [
    cron('0 0 9 * * * *'),
  ],
  enqueues: ['report.generated'],
  virtualEnqueues: ['email.sent'],
  virtualSubscribes: ['report.requested'],
  flows: ['reporting'],
  includeFiles: ['./templates/report.html'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='JavaScript'>

```javascript
import { cron } from 'motia'

export const config = {
  name: 'DailyReport',
  description: 'Generates daily reports at 9 AM',
  triggers: [
    cron('0 0 9 * * * *'),
  ],
  enqueues: ['report.generated'],
  virtualEnqueues: ['email.sent'],
  virtualSubscribes: ['report.requested'],
  flows: ['reporting'],
  includeFiles: ['./templates/report.html'],
}
```

</Tab>
<Tab value='Python'>

```python
config = {
    "name": "DailyReport",
    "description": "Generates daily reports at 9 AM",
    "triggers": [
        {"type": "cron", "expression": "0 0 9 * * * *"},
    ],
    "enqueues": ["report.generated"],
    "virtualEnqueues": ["email.sent"],
    "virtualSubscribes": ["report.requested"],
    "flows": ["reporting"],
    "includeFiles": ["./templates/report.html"],
}
```

</Tab>
</Tabs>

---

### Multi-Trigger Step Config Example

A single step can respond to multiple trigger types:

<Tabs items={['TypeScript', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { StepConfig, http, queue, cron } from 'motia'

export const config = {
  name: 'UserSync',
  description: 'Syncs user data from multiple sources',
  triggers: [
    http('POST', '/users/sync'),
    queue('user.updated'),
    cron('0 0 */6 * * * *'),
  ],
  enqueues: ['user.synced'],
  flows: ['user-management'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='JavaScript'>

```javascript
import { cron } from 'motia'

export const config = {
  name: 'UserSync',
  description: 'Syncs user data from multiple sources',
  triggers: [
    { type: 'http', method: 'POST', path: '/users/sync' },
    { type: 'queue', topic: 'user.updated' },
    cron('0 0 */6 * * * *'),
  ],
  enqueues: ['user.synced'],
  flows: ['user-management'],
}
```

</Tab>
</Tabs>

---

## Handlers

Handlers are the functions that execute your business logic. Use the `Handlers` type with your config for full type safety.

### Handlers Type

```typescript
type Handlers<TConfig extends StepConfig> = (
  input: InferHandlerInput<TConfig>,
  ctx: FlowContext<InferHandlerInput<TConfig>>,
) => Promise<HttpResponse | void>
```

The handler signature is unified -- the `input` type is inferred from the trigger that activated the step. Use `ctx.match()` or `ctx.is` to differentiate between trigger types in multi-trigger steps. Note that `enqueue`, `logger`, `stateManager`, and streams are now standalone imports from `motia` rather than context properties.

---

### HTTP Step Handler

Receives a request, returns a response.

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
import { StepConfig, Handlers, http, enqueue } from 'motia'

export const config = {
  name: 'CreateUser',
  triggers: [http('POST', '/users')],
  enqueues: ['user.created'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }, { traceId }) => {
  const { name, email } = request.body
  const userId = crypto.randomUUID()

  await enqueue({
    topic: 'user.created',
    data: { userId, email }
  })

  return {
    status: 201,
    body: { id: userId, name, email },
    headers: { 'X-Request-ID': traceId }
  }
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
import { enqueue } from 'motia'

export const handler = async ({ request }, { traceId }) => {
  const { name, email } = request.body
  const userId = crypto.randomUUID()

  await enqueue({
    topic: 'user.created',
    data: { userId, email }
  })

  return {
    status: 201,
    body: { id: userId, name, email },
    headers: { 'X-Request-ID': traceId }
  }
}
```

</Tab>
<Tab value='Python'>

```python
import uuid
from typing import Any
from motia import ApiRequest, ApiResponse, FlowContext, http, enqueue

async def handler(request: ApiRequest[Any], ctx: FlowContext[Any]) -> ApiResponse[Any]:
    name = request.body.get("name")
    email = request.body.get("email")
    user_id = str(uuid.uuid4())

    await enqueue({
        "topic": "user.created",
        "data": {"user_id": user_id, "email": email}
    })

    return ApiResponse(
        status=201,
        body={"id": user_id, "name": name, "email": email},
        headers={"X-Request-ID": ctx.trace_id}
    )
```

</Tab>
</Tabs>

---

### Queue Step Handler

Receives queue data, processes it. No return value.

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
import { StepConfig, Handlers, queue, enqueue, logger, stateManager } from 'motia'

export const config = {
  name: 'ProcessOrder',
  triggers: [queue('order.created', { input: z.object({ orderId: z.string(), amount: z.number() }) })],
  enqueues: ['order.processed'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input, ctx) => {
  const data = ctx.getData()
  const { orderId, amount } = data

  logger.info('Processing order', { orderId, amount })

  await stateManager.set('orders', orderId, {
    id: orderId,
    amount,
    status: 'processed'
  })

  await enqueue({
    topic: 'order.processed',
    data: { orderId }
  })
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
import { enqueue, logger, stateManager } from 'motia'

export const handler = async (input, ctx) => {
  const data = ctx.getData()
  const { orderId, amount } = data

  logger.info('Processing order', { orderId, amount })

  await stateManager.set('orders', orderId, {
    id: orderId,
    amount,
    status: 'processed'
  })

  await enqueue({
    topic: 'order.processed',
    data: { orderId }
  })
}
```

</Tab>
<Tab value='Python'>

```python
from motia import enqueue, logger, state_manager

async def handler(input_data, context):
    order_id = input_data.get("order_id")
    amount = input_data.get("amount")

    logger.info("Processing order", {"order_id": order_id, "amount": amount})

    await state_manager.set("orders", order_id, {
        "id": order_id,
        "amount": amount,
        "status": "processed"
    })

    await enqueue({
        "topic": "order.processed",
        "data": {"order_id": order_id}
    })
```

</Tab>
</Tabs>

---

### Cron Step Handler

Runs on a schedule. Uses standalone imports for logging and state management.

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
import { StepConfig, Handlers, cron, logger, stateManager } from 'motia'

export const config = {
  name: 'DailyCleanup',
  triggers: [cron('0 0 0 * * * *')],
  enqueues: [],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input) => {
  logger.info('Running daily cleanup')

  const oldOrders = await stateManager.list('orders')
  const cutoff = Date.now() - (30 * 24 * 60 * 60 * 1000)

  for (const order of oldOrders) {
    if (order.createdAt < cutoff) {
      await stateManager.delete('orders', order.id)
    }
  }
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
import { logger, stateManager } from 'motia'

export const handler = async (input) => {
  logger.info('Running daily cleanup')

  const oldOrders = await stateManager.list('orders')
  const cutoff = Date.now() - (30 * 24 * 60 * 60 * 1000)

  for (const order of oldOrders) {
    if (order.createdAt < cutoff) {
      await stateManager.delete('orders', order.id)
    }
  }
}
```

</Tab>
<Tab value='Python'>

```python
from datetime import datetime, timedelta
from motia import logger, state_manager

async def handler(input_data):
    logger.info("Running daily cleanup")

    old_orders = await state_manager.list("orders")
    cutoff = (datetime.now() - timedelta(days=30)).timestamp()

    for order in old_orders:
        if order.get("created_at") < cutoff:
            await state_manager.delete("orders", order.get("id"))
```

</Tab>
</Tabs>

---

### Multi-Trigger Handler with match()

For steps with multiple triggers, use `ctx.match()` to handle each trigger type:

<Tabs items={['TypeScript', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { StepConfig, Handlers, http, queue, cron, stateManager } from 'motia'

export const config = {
  name: 'UserSync',
  triggers: [
    http('POST', '/users/sync'),
    queue('user.updated'),
    cron('0 0 */6 * * * *'),
  ],
  enqueues: ['user.synced'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input, ctx) => {
  return ctx.match({
    http: async ({ request }) => {
      const { userId } = request.body
      await syncUser(userId)
      return { status: 200, body: { synced: true } }
    },
    queue: async (data) => {
      const payload = ctx.getData()
      await syncUser(payload.userId)
    },
    cron: async () => {
      const allUsers = await stateManager.list('users')
      for (const user of allUsers) {
        await syncUser(user.id)
      }
    },
  })
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
import { stateManager } from 'motia'

export const handler = async (input, ctx) => {
  return ctx.match({
    http: async ({ request }) => {
      const { userId } = request.body
      await syncUser(userId)
      return { status: 200, body: { synced: true } }
    },
    queue: async (data) => {
      const payload = ctx.getData()
      await syncUser(payload.userId)
    },
    cron: async () => {
      const allUsers = await stateManager.list('users')
      for (const user of allUsers) {
        await syncUser(user.id)
      }
    },
  })
}
```

</Tab>
</Tabs>

You can also use `ctx.is` for simpler checks:

```typescript
if (ctx.is.http(input)) {
  return { status: 200, body: { ok: true } }
}
if (ctx.is.queue(input)) {
  const data = ctx.getData()
}
if (ctx.is.cron(input)) {
  // scheduled execution
}
```

---

## Handler Context (FlowContext)

Every handler receives an optional context object (`ctx` in TypeScript/JavaScript, `context` in Python). The `enqueue`, `logger`, `stateManager`, and streams are now standalone imports from `motia` rather than context properties.

```typescript
interface FlowContext<TInput> {
  traceId: string
  trigger: TriggerInfo
  is: {
    queue: (input) => boolean
    http: (input) => boolean
    cron: (input) => boolean
    state: (input) => boolean
    stream: (input) => boolean
  }
  getData: () => ExtractDataPayload<TInput>
  match: <TResult>(handlers: MatchHandlers) => Promise<TResult | void>
}
```

### enqueue

Trigger other Steps by publishing messages to topics. Import directly from `motia`:

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
import { enqueue } from 'motia'

await enqueue({
  topic: 'order.created',
  data: { orderId: '123', total: 99.99 }
})

await enqueue({
  topic: 'order.processing',
  data: { orderId: '123', items: ['item1', 'item2'] },
  messageGroupId: 'user-456'
})
```

**FIFO queues:** When enqueuing to a topic that has a FIFO queue subscriber, you **must** include `messageGroupId`. Messages with the same `messageGroupId` are processed sequentially. Different groups are processed in parallel.

</Tab>
<Tab value='JavaScript'>

```javascript
import { enqueue } from 'motia'

await enqueue({
  topic: 'order.created',
  data: { orderId: '123', total: 99.99 }
})

await enqueue({
  topic: 'order.processing',
  data: { orderId: '123', items: ['item1', 'item2'] },
  messageGroupId: 'user-456'
})
```

**FIFO queues:** When enqueuing to a topic that has a FIFO queue subscriber, you **must** include `messageGroupId`. Messages with the same `messageGroupId` are processed sequentially. Different groups are processed in parallel.

</Tab>
<Tab value='Python'>

```python
from motia import enqueue

await enqueue({
    "topic": "order.created",
    "data": {"order_id": "123", "total": 99.99}
})

await enqueue({
    "topic": "order.processing",
    "data": {"order_id": "123", "items": [...]},
    "messageGroupId": "user-456"
})
```

**FIFO queues:** When enqueuing to a topic that has a FIFO queue subscriber, you **must** include `messageGroupId`. Messages with the same `messageGroupId` are processed sequentially. Different groups are processed in parallel.

</Tab>
</Tabs>

The `data` must match the `input` schema of Steps subscribing to that topic.

---

### logger

Structured logging with automatic trace ID correlation. Import directly from `motia`:

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
import { logger } from 'motia'

logger.info('User created', { userId: '123', email: 'user@example.com' })
logger.warn('Rate limit approaching', { current: 95, limit: 100 })
logger.error('Payment failed', { error: err.message, orderId: '456' })
logger.debug('Cache miss', { key: 'user:123' })
```

</Tab>
<Tab value='JavaScript'>

```javascript
import { logger } from 'motia'

logger.info('User created', { userId: '123', email: 'user@example.com' })
logger.warn('Rate limit approaching', { current: 95, limit: 100 })
logger.error('Payment failed', { error: err.message, orderId: '456' })
logger.debug('Cache miss', { key: 'user:123' })
```

</Tab>
<Tab value='Python'>

```python
from motia import logger

logger.info("User created", {"user_id": "123", "email": "user@example.com"})
logger.warn("Rate limit approaching", {"current": 95, "limit": 100})
logger.error("Payment failed", {"error": str(err), "order_id": "456"})
logger.debug("Cache miss", {"key": "user:123"})
```

</Tab>
</Tabs>

All logs are automatically tagged with:
- Timestamp
- Step name
- Trace ID
- Any metadata you pass

[Learn more about Observability](/docs/development-guide/observability)

---

### stateManager

Persistent key-value storage shared across Steps. Import directly from `motia`:

```typescript
import { stateManager } from 'motia'
```

```typescript
interface InternalStateManager {
  get<T>(groupId: string, key: string): Promise<T | null>
  set<T>(groupId: string, key: string, value: T): Promise<StreamSetResult<T> | null>
  update<T>(groupId: string, key: string, ops: UpdateOp[]): Promise<StreamSetResult<T> | null>
  delete<T>(groupId: string, key: string): Promise<T | null>
  list<T>(groupId: string): Promise<T[]>
  clear(groupId: string): Promise<void>
}

type StreamSetResult<T> = { new_value: T; old_value: T | null }

type UpdateOp =
  | { type: 'set', path: string, value: any }
  | { type: 'merge', path?: string, value: any }
  | { type: 'increment', path: string, by: number }
  | { type: 'decrement', path: string, by: number }
  | { type: 'remove', path: string }
```

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
import { stateManager } from 'motia'

await stateManager.set('users', 'user-123', { name: 'Alice', email: 'alice@example.com' })

const user = await stateManager.get<User>('users', 'user-123')

const allUsers = await stateManager.list<User>('users')

await stateManager.update('users', 'user-123', [
  { type: 'set', path: 'name', value: 'Bob' },
  { type: 'increment', path: 'loginCount', by: 1 },
])

await stateManager.delete('users', 'user-123')

await stateManager.clear('users')
```

</Tab>
<Tab value='JavaScript'>

```javascript
import { stateManager } from 'motia'

await stateManager.set('users', 'user-123', { name: 'Alice', email: 'alice@example.com' })

const user = await stateManager.get('users', 'user-123')

const allUsers = await stateManager.list('users')

await stateManager.update('users', 'user-123', [
  { type: 'set', path: 'name', value: 'Bob' },
  { type: 'increment', path: 'loginCount', by: 1 },
])

await stateManager.delete('users', 'user-123')

await stateManager.clear('users')
```

</Tab>
<Tab value='Python'>

```python
from motia import state_manager

await state_manager.set("users", "user-123", {"name": "Alice", "email": "alice@example.com"})

user = await state_manager.get("users", "user-123")

all_users = await state_manager.list("users")

await state_manager.update("users", "user-123", [
    {"type": "set", "path": "name", "value": "Bob"},
    {"type": "increment", "path": "loginCount", "by": 1},
])

await state_manager.delete("users", "user-123")

await state_manager.clear("users")
```

</Tab>
</Tabs>

**Methods:**

- `stateManager.get(groupId, key)` - Returns the value or `null`
- `stateManager.set(groupId, key, value)` - Stores the value and returns `{ new_value, old_value }`
- `stateManager.update(groupId, key, ops)` - Applies atomic update operations and returns `{ new_value, old_value }`
- `stateManager.delete(groupId, key)` - Removes and returns the value (or `null`)
- `stateManager.list(groupId)` - Returns array of all values in the group
- `stateManager.clear(groupId)` - Removes all items in the group

[Learn more about State](/docs/development-guide/state-management)

---

### Streams

Real-time data channels for pushing updates to connected clients. Stream instances are imported directly from their `.stream.ts` files:

```typescript
import { chatMessagesStream } from './chat-messages.stream'
```

```typescript
interface MotiaStream<TData> {
  get(groupId: string, id: string): Promise<BaseStreamItem<TData> | null>
  set(groupId: string, id: string, data: TData): Promise<StreamSetResult<BaseStreamItem<TData>>>
  delete(groupId: string, id: string): Promise<BaseStreamItem<TData> | null>
  getGroup(groupId: string): Promise<BaseStreamItem<TData>[]>
  update(groupId: string, id: string, data: UpdateOp[]): Promise<StreamSetResult<BaseStreamItem<TData>>>
  send<T>(channel: StateStreamEventChannel, event: StateStreamEvent<T>): Promise<void>
}
```

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
// Import the stream instance from its stream file
import { chatMessagesStream } from './chat-messages.stream'

await chatMessagesStream.set('room-123', 'msg-456', {
  text: 'Hello!',
  author: 'Alice',
  timestamp: new Date().toISOString()
})

const message = await chatMessagesStream.get('room-123', 'msg-456')

const messages = await chatMessagesStream.getGroup('room-123')

await chatMessagesStream.delete('room-123', 'msg-456')

await chatMessagesStream.update('room-123', 'msg-456', [
  { type: 'set', path: 'text', value: 'Updated message' },
])

await chatMessagesStream.send(
  { groupId: 'room-123' },
  { type: 'user.typing', data: { userId: 'alice' } }
)
```

</Tab>
<Tab value='JavaScript'>

```javascript
// Import the stream instance from its stream file
import { chatMessagesStream } from './chat-messages.stream'

await chatMessagesStream.set('room-123', 'msg-456', {
  text: 'Hello!',
  author: 'Alice',
  timestamp: new Date().toISOString()
})

const message = await chatMessagesStream.get('room-123', 'msg-456')

const messages = await chatMessagesStream.getGroup('room-123')

await chatMessagesStream.delete('room-123', 'msg-456')

await chatMessagesStream.update('room-123', 'msg-456', [
  { type: 'set', path: 'text', value: 'Updated message' },
])

await chatMessagesStream.send(
  { groupId: 'room-123' },
  { type: 'user.typing', data: { userId: 'alice' } }
)
```

</Tab>
<Tab value='Python'>

```python
# Import the stream instance from its stream file
from chat_messages_stream import chat_messages_stream

await chat_messages_stream.set("room-123", "msg-456", {
    "text": "Hello!",
    "author": "Alice",
    "timestamp": datetime.now().isoformat()
})

message = await chat_messages_stream.get("room-123", "msg-456")

messages = await chat_messages_stream.get_group("room-123")

await chat_messages_stream.delete("room-123", "msg-456")

await chat_messages_stream.update("room-123", "msg-456", [
    {"type": "set", "path": "text", "value": "Updated message"},
])

await chat_messages_stream.send(
    {"groupId": "room-123"},
    {"type": "user.typing", "data": {"user_id": "alice"}}
)
```

</Tab>
</Tabs>

**Methods:**

- `stream.set(groupId, id, data)` - Create or update an item (returns `{ new_value, old_value }`)
- `stream.get(groupId, id)` - Retrieve an item or `null`
- `stream.getGroup(groupId)` - Get all items in a group
- `stream.delete(groupId, id)` - Remove an item
- `stream.update(groupId, id, ops)` - Apply atomic update operations
- `stream.send(channel, event)` - Send an ephemeral event (e.g., typing indicators, reactions)

[Learn more about Streams](/docs/development-guide/streams)

---

### traceId

Unique ID for tracking requests across Steps.

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
import { logger } from 'motia'

export const handler: Handlers<typeof config> = async ({ request }, { traceId }) => {
  logger.info('Processing request', { traceId })
  return { status: 200, body: { traceId } }
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
import { logger } from 'motia'

export const handler = async ({ request }, { traceId }) => {
  logger.info('Processing request', { traceId })
  return { status: 200, body: { traceId } }
}
```

</Tab>
<Tab value='Python'>

```python
from typing import Any
from motia import ApiRequest, ApiResponse, FlowContext, logger

async def handler(request: ApiRequest[Any], ctx: FlowContext[Any]) -> ApiResponse[Any]:
    logger.info("Processing request", {"trace_id": ctx.trace_id})
    return ApiResponse(status=200, body={"trace_id": ctx.trace_id})
```

</Tab>
</Tabs>

The trace ID is automatically generated for each request and passed through all Steps in the workflow. Use it to correlate logs, state, and events.

---

### trigger

Information about what triggered the current handler execution.

```typescript
interface TriggerInfo {
  type: 'http' | 'queue' | 'cron' | 'state' | 'stream'
}
```

---

### getData

Extract the data payload from the input. Useful in queue and stream handlers.

```typescript
const data = ctx.getData()
```

---

### match

Route execution based on trigger type. See [Multi-Trigger Handler with match()](#multi-trigger-handler-with-match) above.

```typescript
type MatchHandlers<TInput, TResult> = {
  queue?: (input) => Promise<void>
  http?: (request) => Promise<TResult>
  cron?: () => Promise<void>
  state?: (input) => Promise<TResult>
  stream?: (input) => Promise<TResult>
  default?: (input) => Promise<TResult | void>
}
```

---

## Middleware

Intercepts API requests before and after the handler.

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
import { ApiMiddleware } from 'motia'

export const authMiddleware: ApiMiddleware = async ({ request }, ctx, next) => {
  const token = request.headers.authorization

  if (!token) {
    return { status: 401, body: { error: 'Unauthorized' } }
  }

  return await next()
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
const authMiddleware = async ({ request }, ctx, next) => {
  const token = request.headers.authorization

  if (!token) {
    return { status: 401, body: { error: 'Unauthorized' } }
  }

  return await next()
}
```

</Tab>
<Tab value='Python'>

```python
async def auth_middleware(request, context, next_fn):
    token = request.headers.get("authorization")

    if not token:
        return {"status": 401, "body": {"error": "Unauthorized"}}

    return await next_fn()
```

</Tab>
</Tabs>

**Parameters:**
- `{ request, response }` - The `MotiaHttpArgs` object containing request and response
- `ctx` - Context object
- `next` - Function to call the next middleware/handler

**Returns:** Response object

[Learn more about Middleware](/docs/development-guide/middleware)

---

## Handler Input

TypeScript and JavaScript handlers receive `MotiaHttpArgs` (containing `request` and `response`).
Python handlers use `ApiRequest` for standard responses and only use `MotiaHttpArgs` for streaming/SSE handlers.

```typescript
interface MotiaHttpArgs<TBody = unknown> {
  request: MotiaHttpRequest<TBody>
  response: MotiaHttpResponse
}

interface MotiaHttpRequest<TBody = unknown> {
  pathParams: Record<string, string>
  queryParams: Record<string, string | string[]>
  body: TBody
  headers: Record<string, string | string[]>
  method: string
  requestBody: ChannelReader
}

type MotiaHttpResponse = {
  status: (statusCode: number) => void
  headers: (headers: Record<string, string>) => void
  stream: NodeJS.WritableStream
  close: () => void
}
```

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
export const handler: Handlers<typeof config> = async ({ request }, ctx) => {
  const userId = request.pathParams.id
  const page = request.queryParams.page
  const limit = request.queryParams.limit
  const { name, email } = request.body
  const auth = request.headers.authorization

  return { status: 200, body: { userId, name } }
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
export const handler = async ({ request }, ctx) => {
  const userId = request.pathParams.id
  const page = request.queryParams.page
  const limit = request.queryParams.limit
  const { name, email } = request.body
  const auth = request.headers.authorization

  return { status: 200, body: { userId, name } }
}
```

</Tab>
<Tab value='Python'>

```python
from typing import Any
from motia import ApiRequest, ApiResponse, FlowContext

async def handler(request: ApiRequest[Any], ctx: FlowContext[Any]) -> ApiResponse[Any]:
    user_id = request.path_params.get("id")
    page = request.query_params.get("page")
    limit = request.query_params.get("limit")
    name = request.body.get("name")
    email = request.body.get("email")
    auth = request.headers.get("authorization")

    return ApiResponse(status=200, body={"user_id": user_id, "name": name})
```

</Tab>
</Tabs>

**`request` fields:**
- `pathParams` / `path_params` (Python) - Object with path parameters (e.g., `:id` from `/users/:id`)
- `queryParams` / `query_params` (Python) - Object with query string params (values can be string or array)
- `body` - Parsed request body (validated against `bodySchema` if defined)
- `headers` - Object with request headers (values can be string or array)
- `method` - HTTP method string
- `requestBody` / `request_body` (Python) - Raw request body stream (for SSE / streaming)

**`response` fields (for SSE / streaming):**
- `status(code)` - Set HTTP status code
- `headers(headers)` - Set response headers
- `stream` / `writer.stream` (Python) - Writable stream for sending data
- `close()` - Close the response stream

---

## Response Object (ApiResponse)

HTTP handlers must return an object with these fields.

```typescript
type ApiResponse<TStatus extends number = number, TBody = any> = {
  status: TStatus
  headers?: Record<string, string>
  body: TBody
}
```

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
return {
  status: 200,
  body: { id: '123', name: 'Alice' },
  headers: {
    'Cache-Control': 'max-age=3600',
    'X-Custom-Header': 'value'
  }
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
return {
  status: 200,
  body: { id: '123', name: 'Alice' },
  headers: {
    'Cache-Control': 'max-age=3600',
    'X-Custom-Header': 'value'
  }
}
```

</Tab>
<Tab value='Python'>

```python
from motia import ApiResponse

return ApiResponse(
    status=200,
    body={"id": "123", "name": "Alice"},
    headers={
        "Cache-Control": "max-age=3600",
        "X-Custom-Header": "value"
    }
)
```

</Tab>
</Tabs>

**Fields:**
- `status` - HTTP status code (200, 201, 400, 404, 500, etc.)
- `body` - Response data (will be JSON-encoded automatically)
- `headers` - Optional custom headers

---

## Server-Sent Events (SSE)

HTTP handlers can stream responses using Server-Sent Events. Instead of returning a response object, use the `response` object from `MotiaHttpArgs` to set headers and write to the stream.

<Tabs items={['TypeScript', 'Python']}>
<Tab value='TypeScript'>

```typescript title="src/sse-example.step.ts"
import { type Handlers, http, type StepConfig, logger } from 'motia'

export const config = {
  name: 'SSE Example',
  description: 'Streams data back to the client as SSE',
  flows: ['sse-example'],
  triggers: [http('POST', '/sse')],
  enqueues: [],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request, response }) => {
  logger.info('SSE request received')

  response.status(200)
  response.headers({
    'content-type': 'text/event-stream',
    'cache-control': 'no-cache',
    connection: 'keep-alive',
  })

  const chunks: string[] = []
  for await (const chunk of request.requestBody.stream) {
    chunks.push(Buffer.from(chunk).toString('utf-8'))
  }
  const body = chunks.join('').trim()

  const items = ['alpha', 'bravo', 'charlie']
  for (const item of items) {
    response.stream.write(`event: item\ndata: ${JSON.stringify({ item })}\n\n`)
    await new Promise((resolve) => setTimeout(resolve, 500))
  }

  response.stream.write(`event: done\ndata: ${JSON.stringify({ total: items.length })}\n\n`)
  response.close()
}
```

</Tab>
<Tab value='Python'>

```python title="src/sse_example_step.py"
import asyncio
import json
from typing import Any

from motia import MotiaHttpArgs, FlowContext, http, logger

config = {
    "name": "SSE Example",
    "description": "Streams data back to the client as SSE",
    "flows": ["sse-example"],
    "triggers": [
        http("POST", "/sse"),
    ],
    "enqueues": [],
}

async def handler(args: MotiaHttpArgs[Any], ctx: FlowContext[Any]) -> None:
    request = args.request
    response = args.response

    logger.info("SSE request received")

    await response.status(200)
    await response.headers({
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        "connection": "keep-alive",
    })

    raw_chunks: list[str] = []
    async for chunk in request.request_body.stream:
        if isinstance(chunk, bytes):
            raw_chunks.append(chunk.decode("utf-8", errors="replace"))
        else:
            raw_chunks.append(str(chunk))
    payload = "".join(raw_chunks).strip()

    items = ["alpha", "bravo", "charlie"]
    for item in items:
        response.writer.stream.write(f"event: item\ndata: {json.dumps({'item': item})}\n\n".encode("utf-8"))
        await asyncio.sleep(0.5)

    response.writer.stream.write(f"event: done\ndata: {json.dumps({'total': len(items)})}\n\n".encode("utf-8"))
    response.close()
```

</Tab>
</Tabs>

**Key differences from standard HTTP handlers:**
- Destructure both `request` and `response` from the first argument
- Use `response.status()` and `response.headers()` to set the status and headers
- Write SSE-formatted data to `response.stream` (TS/JS) or `response.writer.stream` (Python)
- Call `response.close()` when done streaming
- Do **not** return a response object — the response is streamed directly

---

## Stream Configuration

Define real-time data streams for your app.

```typescript
interface StreamConfig {
  name: string
  schema: StepSchemaInput
  baseConfig: { storageType: 'default' }
  onJoin?: (subscription, context, authContext?) => StreamJoinResult
  onLeave?: (subscription, context, authContext?) => void
}
```

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript title="src/chat-messages.stream.ts"
import { StreamConfig } from 'motia'
import { z } from 'zod'

export const config: StreamConfig = {
  name: 'chatMessages',
  schema: z.object({
    text: z.string(),
    author: z.string(),
    timestamp: z.string()
  }),
  baseConfig: {
    storageType: 'default'
  }
}
```

</Tab>
<Tab value='JavaScript'>

```javascript title="src/chat-messages.stream.js"
import { z } from 'zod'

export const config = {
  name: 'chatMessages',
  schema: z.object({
    text: z.string(),
    author: z.string(),
    timestamp: z.string()
  }),
  baseConfig: {
    storageType: 'default'
  }
}
```

</Tab>
<Tab value='Python'>

```python title="src/chat_messages_stream.py"
from pydantic import BaseModel

class ChatMessage(BaseModel):
    text: str
    author: str
    timestamp: str

config = {
    "name": "chatMessages",
    "schema": ChatMessage.model_json_schema(),
    "baseConfig": {
        "storageType": "default"
    }
}
```

</Tab>
</Tabs>

**Fields:**
- `name` - Unique stream name (the stream instance is imported from the stream file)
- `schema` - Zod schema (TS/JS) or JSON Schema (Python) for data validation
- `baseConfig.storageType` - Always `'default'` (custom storage coming soon)
- `onJoin` - Optional callback when a client subscribes
- `onLeave` - Optional callback when a client unsubscribes

File naming:
- TypeScript/JavaScript: `*.stream.ts` or `*.stream.js`
- Python: `*_stream.py`

---

## CLI Commands

For CLI usage, see the [CLI Reference](/docs/development-guide/cli).

---

## Common Patterns

### Enqueue Types

You can enqueue topics as strings or objects with labels.

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
<Tab value='TypeScript'>

```typescript
enqueues: ['user.created', 'email.sent']

enqueues: [
  { topic: 'order.approved', label: 'Auto-approved' },
  { topic: 'order.rejected', label: 'Requires review', conditional: true }
]
```

</Tab>
<Tab value='JavaScript'>

```javascript
enqueues: ['user.created', 'email.sent']

enqueues: [
  { topic: 'order.approved', label: 'Auto-approved' },
  { topic: 'order.rejected', label: 'Requires review', conditional: true }
]
```

</Tab>
<Tab value='Python'>

```python
"enqueues": ["user.created", "email.sent"]

"enqueues": [
    {"topic": "order.approved", "label": "Auto-approved"},
    {"topic": "order.rejected", "label": "Requires review", "conditional": True}
]
```

</Tab>
</Tabs>

The `label` and `conditional` fields are for iii development console visualization only. They don't affect execution.

---

### Query Parameters

Document query params for the iii development console.

```typescript
queryParams: [
  { name: 'page', description: 'Page number for pagination' },
  { name: 'limit', description: 'Number of items per page' },
  { name: 'sort', description: 'Sort field (e.g., createdAt, name)' }
]
```

This metadata is used by the iii development console.

---

### Include Files

Bundle files with your Step (useful for templates, assets, binaries).

```typescript
includeFiles: [
  './templates/email.html',
  './assets/*.png',
  '../../lib/stockfish'
]
```

Files are copied into the deployment bundle and accessible at runtime.

---

## What's Next?

<Cards>
  <Card href="/docs/concepts/steps" title="Steps">
    Learn how to build with Steps
  </Card>

  <Card href="/docs/development-guide/state-management" title="State Management">
    Deep dive into the State API
  </Card>

  <Card href="/docs/development-guide/streams" title="Streams">
    Real-time streaming guide
  </Card>

  <Card href="/docs/development-guide/authentication" title="Authentication">
    Authentication patterns for HTTP and stream endpoints
  </Card>

  <Card href="https://github.com/MotiaDev/motia-examples" title="Examples">
    See these APIs in action
  </Card>
</Cards>
