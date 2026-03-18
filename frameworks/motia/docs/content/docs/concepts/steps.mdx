---
title: Steps
description: One primitive to build any backend. Simple, composable, and multi-language.
---

<video controls className="mb-8 w-full rounded-xl" poster="https://assets.motia.dev/images/gifs/v1/3-motia-steps.gif">
  <source src="https://assets.motia.dev/videos/mp4/site/v1/3-motia-steps.mp4" type="video/mp4" />
</video>

## One Primitive for Any Backend

A **Step** is the core primitive in Motia. Instead of juggling separate frameworks for APIs, background jobs, queues, or workflows, you define everything in one place:   **how it runs, when it runs, where it runs, and what it does.**

Every Step file contains two parts:

- **Config** → defines when and how the Step runs, and gives it a unique `name`
- **Handler** → the function that executes your business logic

Motia automatically discovers any file ending in `.step.ts`, `.step.js`, or `_step.py` from your `src/` directory.
The filename pattern tells Motia to load it, and the `name` in the `config` uniquely identifies the Step inside your system.

<Callout type="info">
**Flexible Organization** - Steps can be placed anywhere within your `src/` directory. Motia discovers them automatically regardless of how deeply nested they are.
</Callout>

---

## The Simplest Example

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```ts title="src/hello.step.ts"
import { type Handlers, type StepConfig, logger } from 'motia'
import { z } from 'zod'

export const config = {
  name: 'HelloStep',
  description: 'Hello endpoint',
  triggers: [
    { type: 'http', path: '/hello', method: 'GET', responseSchema: { 200: z.object({ message: z.string() }) } },
  ],
  enqueues: [],
  flows: ['my-flow'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  logger.info('Hello endpoint called')
  return { status: 200, body: { message: 'Hello world!' } }
}
```

</Tab>
<Tab value='Python'>

```python title="src/hello_step.py"
from typing import Any
from motia import ApiRequest, ApiResponse, http, logger

config = {
    "name": "HelloStep",
    "description": "Hello endpoint",
    "triggers": [
        http("GET", "/hello"),
    ],
    "enqueues": [],
    "flows": ["my-flow"]
}

async def handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
    logger.info("Hello endpoint called")
    return ApiResponse(status=200, body={"message": "Hello world!"})
```

</Tab>
<Tab value='JavaScript'>

```js title="src/hello.step.js"
import { logger } from 'motia'
import { z } from 'zod'

export const config = {
  name: 'HelloStep',
  description: 'Hello endpoint',
  triggers: [
    { type: 'http', path: '/hello', method: 'GET', responseSchema: { 200: z.object({ message: z.string() }) } },
  ],
  enqueues: [],
  flows: ['my-flow'],
}

export const handler = async ({ request }) => {
  logger.info('Hello endpoint called')
  return { status: 200, body: { message: 'Hello world!' } }
}
```

</Tab>
</Tabs>

That's all you need to make a running API endpoint.
Motia will auto-discover this file and wire it into your backend.

---

## Steps Work Together: Enqueue + Queue

Steps aren't isolated. They communicate by **enqueuing** messages that other Steps listen for via **queue triggers**.
This is the core of how you build backends with Motia.

### Example Flow: API Step → Queue Step

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```ts title="src/send-message.step.ts"
import { type Handlers, type StepConfig, enqueue } from 'motia'
import { z } from 'zod'

export const config = {
  name: 'SendMessage',
  description: 'Sends a message',
  triggers: [
    { type: 'http', path: '/messages', method: 'POST' },
  ],
  enqueues: ['message.sent'],
  flows: ['messaging'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  await enqueue({
    topic: 'message.sent',
    data: { text: request.body.text }
  })
  return { status: 200, body: { ok: true } }
}
```

```ts title="src/process-message.step.ts"
import { type Handlers, type StepConfig, logger, enqueue } from 'motia'
import { z } from 'zod'

export const config = {
  name: 'ProcessMessage',
  description: 'Processes messages in background',
  triggers: [
    { type: 'queue', topic: 'message.sent', input: z.object({ text: z.string() }) },
  ],
  enqueues: ['message.processed'],
  flows: ['messaging'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input) => {
  logger.info('Processing message', input)
  await enqueue({ topic: 'message.processed', data: input })
}
```
</Tab>

<Tab value='Python'>

```python title="send_message_step.py"
from typing import Any
from motia import ApiRequest, ApiResponse, enqueue, http

config = {
    "name": "SendMessage",
    "description": "Sends a message",
    "triggers": [
        http("POST", "/messages"),
    ],
    "enqueues": ["message.sent"],
    "flows": ["messaging"]
}

async def handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
    await enqueue({
        "topic": "message.sent",
        "data": {"text": request.body["text"]}
    })
    return ApiResponse(status=200, body={"ok": True})
```

```python title="process_message_step.py"
from motia import enqueue, logger

config = {
    "name": "ProcessMessage",
    "description": "Processes messages in background",
    "triggers": [
        {"type": "queue", "topic": "message.sent"}
    ],
    "enqueues": ["message.processed"],
    "flows": ["messaging"]
}

async def handler(input):
    logger.info("Processing message", {"input": input})
    await enqueue({"topic": "message.processed", "data": input})
```
</Tab>

<Tab value='JavaScript'>

```js title="src/send-message.step.js"
import { enqueue } from 'motia'

export const config = {
  name: 'SendMessage',
  description: 'Sends a message',
  triggers: [
    { type: 'http', path: '/messages', method: 'POST' },
  ],
  enqueues: ['message.sent'],
  flows: ['messaging'],
}

export const handler = async ({ request }) => {
  await enqueue({
    topic: 'message.sent',
    data: { text: request.body.text }
  })
  return { status: 200, body: { ok: true } }
}
```

```js title="src/process-message.step.js"
import { logger, enqueue } from 'motia'

export const config = {
  name: 'ProcessMessage',
  description: 'Processes messages in background',
  triggers: [
    { type: 'queue', topic: 'message.sent' },
  ],
  enqueues: ['message.processed'],
  flows: ['messaging'],
}

export const handler = async (input) => {
  logger.info('Processing message', input)
  await enqueue({ topic: 'message.processed', data: input })
}
```
</Tab>
</Tabs>

With just two files, you have an **API endpoint** that triggers an **event-driven workflow**.

---

## Triggers

<div id="triggers-http"></div>
<div id="triggers-queue"></div>
<div id="triggers-cron"></div>

Every Step has a `triggers` array that defines **how it triggers**:

| Type | When it runs | Use case |
|------|--------------|----------|
| `http` | HTTP request | REST APIs, webhooks |
| `queue` | Message enqueued | Background jobs, workflows |
| `cron` | Schedule | Cleanup, reports, reminders |
| `state` | State change | React to data changes |
| `stream` | Stream event | Real-time data processing |

The iii development console lets you browse all registered triggers, test HTTP endpoints directly, and inspect their configuration:

![Triggers view in the iii Console](/console/triggers-detail.png)

<Tabs items={['HTTP', 'Queue', 'Cron', 'State', 'Stream']} groupId="triggers" updateAnchor defaultIndex={0}>
  <Tab id="triggers-http" value="HTTP">

### HTTP Trigger

Runs when an HTTP request hits the path.

**Example:**

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
  <Tab value="TypeScript">
    ```typescript
    import { type Handlers, type StepConfig, logger } from 'motia'
    import { z } from 'zod'

    export const config = {
      name: 'GetUser',
      description: 'Get user by ID',
      triggers: [
        { type: 'http', path: '/users/:id', method: 'GET' },
      ],
      enqueues: [],
      flows: ['users'],
    } as const satisfies StepConfig

    export const handler: Handlers<typeof config> = async ({ request }) => {
      const userId = request.pathParams.id
      logger.info('Getting user', { userId })
      return { status: 200, body: { id: userId, name: 'John' } }
    }
    ```
  </Tab>
  <Tab value="JavaScript">
    ```javascript
    import { logger } from 'motia'

    export const config = {
      name: 'GetUser',
      description: 'Get user by ID',
      triggers: [
        { type: 'http', path: '/users/:id', method: 'GET' },
      ],
      enqueues: [],
      flows: ['users'],
    }

    export const handler = async ({ request }) => {
      const userId = request.pathParams.id
      logger.info('Getting user', { userId })
      return { status: 200, body: { id: userId, name: 'John' } }
    }
    ```
  </Tab>
  <Tab value="Python">
    ```python
    from typing import Any
    from motia import ApiRequest, ApiResponse, http, logger

    config = {
        "name": "GetUser",
        "description": "Get user by ID",
        "triggers": [
            http("GET", "/users/:id"),
        ],
        "enqueues": [],
        "flows": ["users"]
    }

    async def handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
        user_id = request.path_params.get("id")
        logger.info("Getting user", {"userId": user_id})
        return ApiResponse(status=200, body={"id": user_id, "name": "John"})
    ```
  </Tab>
</Tabs>

**Config:**

| Property | Description |
|----------|-------------|
| `name` | Unique identifier |
| `triggers` | Array with `{ type: 'http', path, method }` |
| `path` | URL path (supports `:params`) |
| `method` | GET, POST, PUT, DELETE |
| `bodySchema` | Validate request body |

**Handler:** `handler({ request, response }, ctx)`

- `request` - Request with `body`, `headers`, `pathParams`, `queryParams`, `method`, `requestBody`
- `response` - Response object for streaming (SSE): `status()`, `headers()`, `stream`, `close()`
- `ctx` - Context with `traceId`, `trigger`, `is`, `getData`, `match`
- Returns `{ status, body, headers? }` for standard responses, or use `response` for streaming (SSE)

</Tab>

  <Tab id="triggers-queue" value="Queue">

### Queue Trigger

Runs when a message is enqueued to a topic. Use for background tasks.

**Example:**

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
  <Tab value="TypeScript">
    ```typescript
    import { type Handlers, type StepConfig, logger, enqueue } from 'motia'
    import { z } from 'zod'

    export const config = {
      name: 'ProcessMessage',
      description: 'Processes messages in background',
      triggers: [
        { type: 'queue', topic: 'message.sent', input: z.object({ text: z.string() }) },
      ],
      enqueues: ['message.processed'],
      flows: ['messaging'],
    } as const satisfies StepConfig

    export const handler: Handlers<typeof config> = async (input) => {
      logger.info('Processing message:', input)
      await enqueue({ topic: 'message.processed', data: input })
    }
    ```
  </Tab>
  <Tab value="JavaScript">
    ```javascript
    import { logger, enqueue } from 'motia'

    export const config = {
      name: 'ProcessMessage',
      description: 'Processes messages in background',
      triggers: [
        { type: 'queue', topic: 'message.sent' },
      ],
      enqueues: ['message.processed'],
      flows: ['messaging'],
    }

    export const handler = async (input) => {
      logger.info('Processing message:', input)
      await enqueue({ topic: 'message.processed', data: input })
    }
    ```
  </Tab>
  <Tab value="Python">
    ```python
    from motia import enqueue, logger

    config = {
        "name": "ProcessMessage",
        "description": "Processes messages in background",
        "triggers": [
            {"type": "queue", "topic": "message.sent"}
        ],
        "enqueues": ["message.processed"],
        "flows": ["messaging"]
    }

    async def handler(input):
        logger.info("Processing message:", {"input": input})
        await enqueue({"topic": "message.processed", "data": input})
    ```
  </Tab>
</Tabs>

**Config:**

| Property | Description |
|----------|-------------|
| `name` | Unique identifier |
| `triggers` | Array with `{ type: 'queue', topic }` |
| `topic` | Topic to listen for messages on |
| `enqueues` | Topics this step can enqueue to |
| `input` | Validate input data |

**Handler:** `handler(input, ctx)`

- `input` - Data from the enqueued message
- `ctx` - Context with `traceId`, `trigger`, `is`, `getData`, `match`

</Tab>

  <Tab id="triggers-cron" value="Cron">

### Cron Trigger

Runs on a schedule. Use for periodic tasks.

**Example:**

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
  <Tab value="TypeScript">
    ```typescript
    import { type Handlers, type StepConfig, cron, logger } from 'motia'

    export const config = {
      name: 'DailyCleanup',
      description: 'Runs daily cleanup',
      triggers: [
        cron('0 0 0 * * * *'),
      ],
      enqueues: [],
      flows: ['maintenance'],
    } as const satisfies StepConfig

    export const handler: Handlers<typeof config> = async (input) => {
      logger.info('Running daily cleanup')
    }
    ```
  </Tab>
  <Tab value="JavaScript">
    ```javascript
    import { cron, logger } from 'motia'

    export const config = {
      name: 'DailyCleanup',
      description: 'Runs daily cleanup',
      triggers: [
        cron('0 0 0 * * * *'),
      ],
      enqueues: [],
      flows: ['maintenance'],
    }

    export const handler = async (input) => {
      logger.info('Running daily cleanup')
    }
    ```
  </Tab>
  <Tab value="Python">
    ```python
    config = {
        "name": "DailyCleanup",
        "description": "Runs daily cleanup",
        "triggers": [
            {"type": "cron", "expression": "0 0 0 * * * *"}
        ],
        "enqueues": [],
        "flows": ["maintenance"]
    }

    from motia import logger

    async def handler(input):
        logger.info("Running daily cleanup")
    ```
  </Tab>
</Tabs>

**Config:**

| Property | Description |
|----------|-------------|
| `name` | Unique identifier |
| `triggers` | Array with `cron(expression)` helper |
| `expression` | Cron expression (e.g., `'0 0 0 * * * *'`) |

**Handler:** `handler(input, ctx)`

- `ctx` - Context with `traceId`, `trigger`, `is`, `getData`, `match`

The iii engine uses a **7-field cron expression** that includes seconds and an optional year:

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

| Expression | Runs |
|---|---|
| `0 * * * * * *` | Every minute |
| `0 0 * * * * *` | Every hour |
| `0 0 0 * * * *` | Daily at midnight |
| `0 0 9 * * 1 *` | Monday at 9 AM |
| `0 */5 * * * * *` | Every 5 minutes |

</Tab>

  <Tab value="State">

### State Trigger

Runs when state data changes. Use for reactive workflows that respond to data mutations without polling.

**Example:**

```typescript
import { logger, enqueue } from 'motia'
import type { Handlers, StepConfig, StateTriggerInput } from 'motia'

export const config = {
  name: 'OnAllStepsComplete',
  description: 'Triggers when all parallel steps finish',
  triggers: [
    {
      type: 'state',
      condition: (input: StateTriggerInput<{ totalSteps: number; completedSteps: number }>) => {
        return (
          input.group_id === 'tasks' &&
          !!input.new_value &&
          input.new_value.totalSteps === input.new_value.completedSteps
        )
      },
    },
  ],
  enqueues: ['all-steps-done'],
  flows: ['parallel-merge'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input) => {
  logger.info('All steps complete', { itemId: input.item_id })
  await enqueue({ topic: 'all-steps-done', data: { taskId: input.item_id } })
}
```

**Handler input:** The handler receives a `StateTriggerInput` with `new_value`, `old_value`, `item_id`, and `group_id`.

[Learn more about State Triggers](/docs/advanced-features/reactive-triggers)

</Tab>

  <Tab value="Stream">

### Stream Trigger

Runs when a stream item is created, updated, or deleted. Use for reacting to real-time data changes.

**Example:**

```typescript
import { logger, enqueue } from 'motia'
import type { Handlers, StepConfig, StreamWrapperMessage } from 'motia'

export const config = {
  name: 'OnDeploymentUpdate',
  description: 'Reacts to deployment stream changes',
  triggers: [
    {
      type: 'stream',
      streamName: 'deployment',
      groupId: 'data',
      condition: (input: StreamWrapperMessage) => input.event.type === 'update',
    },
  ],
  enqueues: ['deployment-updated'],
  flows: ['deployments'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input) => {
  logger.info('Deployment updated', { streamName: input.streamName, groupId: input.groupId })
  await enqueue({ topic: 'deployment-updated', data: input.event.data })
}
```

**Handler input:** The handler receives a `StreamWrapperMessage` with `streamName`, `groupId`, `id`, `timestamp`, and an `event` object containing `type` (`create`, `update`, `delete`, or `event`) and `data`.

[Learn more about Stream Triggers](/docs/advanced-features/reactive-triggers)

</Tab>
</Tabs>

---

## Context Object

Every handler receives a `ctx` object with these tools:

| Property | Description |
|----------|-------------|
| `traceId` | Unique ID for tracing requests & workflows |
| `trigger` | Info about which trigger activated this handler |
| `is` | Type guards for trigger types (is.queue, is.http, is.cron) |
| `getData` | Extract data payload regardless of trigger type |
| `match` | Pattern match on trigger type for multi-trigger steps |

<Callout type="info">
**Standalone imports** — `logger`, `enqueue`, `stateManager`, and streams are now imported directly from `'motia'` or from `.stream.ts` files, rather than accessed via `ctx`.
</Callout>

---

## Core Functionality

### State -- Persistent Data

Key-value storage shared across Steps and workflows.

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```ts
import { stateManager } from 'motia'

const result = await stateManager.set('settings', 'preferences', { theme: 'dark' })
const prefs = await stateManager.get('settings', 'preferences')
```

</Tab>
<Tab value='Python'>

```python
from motia import state_manager

result = await state_manager.set("settings", "preferences", {"theme": "dark"})
prefs = await state_manager.get("settings", "preferences")
```

</Tab>
<Tab value='JavaScript'>

```js
import { stateManager } from 'motia'

const result = await stateManager.set('settings', 'preferences', { theme: 'dark' })
const prefs = await stateManager.get('settings', 'preferences')
```

</Tab>
</Tabs>

`stateManager.set` returns `{ new_value, old_value }`. Use `stateManager.update` for atomic updates with `UpdateOp[]`.

[Learn more about State Management](/docs/development-guide/state-management)

### Logging -- Structured & Contextual

For debugging, monitoring, and observability.

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```ts
import { logger } from 'motia'

logger.info('Processing user', { userId: '123' })
```

</Tab>
<Tab value='Python'>

```python
from motia import logger

logger.info("Processing user", {"userId": "123"})
```

</Tab>
<Tab value='JavaScript'>

```js
import { logger } from 'motia'

logger.info('Processing user', { userId: '123' })
```

</Tab>
</Tabs>

[Learn more about Observability](/docs/development-guide/observability)

### Streams -- Real-Time Data

Push updates directly to connected clients.

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```ts
import { chatStream } from './chat.stream'

await chatStream.set('room-123', 'msg-456', { text: 'Hello!' })
```

</Tab>
<Tab value='Python'>

```python
from chat_stream import chat_stream

await chat_stream.set("room-123", "msg-456", {"text": "Hello!"})
```

</Tab>
<Tab value='JavaScript'>

```js
import { chatStream } from './chat.stream'

await chatStream.set('room-123', 'msg-456', { text: 'Hello!' })
```

</Tab>
</Tabs>

[Learn more about Streams](/docs/development-guide/streams)

### Flows -- Visualize in the iii Development Console

Group Steps together for diagram visualization in the iii development console.

![Flow diagram in the iii Console](/console/flow-view.png)

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```ts
export const config = {
  name: 'CreateOrder',
  description: 'Creates a new order',
  triggers: [
    { type: 'http', path: '/orders', method: 'POST' },
  ],
  enqueues: [],
  flows: ['order-management'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='Python'>

```python
config = {
    "name": "CreateOrder",
    "description": "Creates a new order",
    "triggers": [
        {"type": "http", "path": "/orders", "method": "POST"}
    ],
    "enqueues": [],
    "flows": ["order-management"]
}
```

</Tab>
<Tab value='JavaScript'>

```js
export const config = {
  name: 'CreateOrder',
  description: 'Creates a new order',
  triggers: [
    { type: 'http', path: '/orders', method: 'POST' },
  ],
  enqueues: [],
  flows: ['order-management'],
}
```

</Tab>
</Tabs>

[Learn more about Flows](/docs/development-guide/flows)

### Queue Config -- Configure Queue Steps

Customize retry behavior, concurrency, and backoff for Queue Steps.

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```ts
export const config = {
  name: 'SendEmail',
  description: 'Send email with retries',
  triggers: [
    {
      type: 'queue',
      topic: 'email.requested',
      config: { maxRetries: 5, visibilityTimeout: 60 }
    },
  ],
  enqueues: [],
  flows: ['email'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='Python'>

```python
config = {
    "name": "SendEmail",
    "description": "Send email with retries",
    "triggers": [
        {
            "type": "queue",
            "topic": "email.requested",
            "config": {"maxRetries": 5, "visibilityTimeout": 60}
        }
    ],
    "enqueues": [],
    "flows": ["email"]
}
```

</Tab>
<Tab value='JavaScript'>

```js
export const config = {
  name: 'SendEmail',
  description: 'Send email with retries',
  triggers: [
    {
      type: 'queue',
      topic: 'email.requested',
      config: { maxRetries: 5, visibilityTimeout: 60 }
    },
  ],
  enqueues: [],
  flows: ['email'],
}
```

</Tab>
</Tabs>

[Learn more about Queue Config](/docs/development-guide/queue-config)

---

## Multi-Trigger Steps

A single Step can respond to multiple trigger types. For example, a Step can be activated by both an HTTP request and a queue message:

```typescript
export const config = {
  name: 'ProcessOrder',
  triggers: [
    { type: 'http', method: 'POST', path: '/orders/manual' },
    { type: 'queue', topic: 'order.created' },
    { type: 'cron', expression: '0 0 0 * * * *' },
  ],
  enqueues: ['order.processed'],
  flows: ['orders'],
} as const satisfies StepConfig
```

Use `ctx.match()` to handle each trigger type differently, or `ctx.getData()` to extract the data payload regardless of trigger type.

[Learn more about Multi-Trigger Steps](/docs/advanced-features/multi-trigger-steps)

---

## Remember

- **Steps are just files.** Export a `config` and `handler`.
- Motia auto-discovers and connects them.
- Combine Steps with **enqueue + queue triggers** to build APIs, workflows, background jobs, or entire systems.

---

## What's Next?

<Card title="Start building" href="/docs/getting-started/quick-start">
  Steps are all you need to know to start building. Go to the Quickstart and start building right away.
</Card>

[Explore examples](https://github.com/MotiaDev/motia-examples)
