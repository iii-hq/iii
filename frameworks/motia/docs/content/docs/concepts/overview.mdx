---
title: Overview
description: Build production-grade backends with a single primitive - APIs, background jobs, workflows, and AI agents unified
---

**Build production-grade backends with a single primitive.**

Motia is a unified backend framework that combines APIs, background jobs, durable workflows, AI agents, streaming, and observability around one core primitive: **the Step**.

Want an API? That's a Step.
Need a background job? That's a Step.
Scheduled task? Also a Step.

Write each Step in whatever language makes sense — TypeScript, Python, or JavaScript. Each language runtime runs independently, managed by the iii engine, and they all share the same state and communicate through queued messages.

## Naming Clarification: Motia vs iii

| Name | Role |
|---|---|
| **Motia** | The framework/SDK you import in application code (`import { step, enqueue, stateManager } from 'motia'`) |
| **iii** | The runtime engine that runs infrastructure modules (queue, state, stream, cron, HTTP, observability) |

In short: **you write Motia code, and iii runs it**.

## How It Works

Every Step is just a file with two parts:

**1. Config** → When and how it runs
**2. Handler** → What it does

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript title="src/my-step.step.ts"
import { type Handlers, type StepConfig, enqueue, logger } from 'motia'

export const config = {
  name: 'MyStep',
  description: 'Handles incoming requests',
  triggers: [
    { type: 'http', path: '/endpoint', method: 'POST' },
  ],
  enqueues: ['task.done'],
  flows: ['my-flow'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  logger.info('Processing request')

  await enqueue({
    topic: 'task.done',
    data: { result: 'success' }
  })

  return { status: 200, body: { success: true } }
}
```

</Tab>
<Tab value='Python'>

```python title="src/my_step.py"
from typing import Any
from motia import ApiRequest, ApiResponse, http, logger, enqueue

config = {
    "name": "MyStep",
    "description": "Handles incoming requests",
    "triggers": [
        http("POST", "/endpoint"),
    ],
    "enqueues": ["task.done"],
    "flows": ["my-flow"]
}

async def handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
    logger.info("Processing request")

    await enqueue({
        "topic": "task.done",
        "data": {"result": "success"}
    })

    return ApiResponse(status=200, body={"success": True})
```

</Tab>
<Tab value='JavaScript'>

```javascript title="src/my-step.step.js"
import { enqueue, logger } from 'motia'

export const config = {
  name: 'MyStep',
  description: 'Handles incoming requests',
  triggers: [
    { type: 'http', path: '/endpoint', method: 'POST' },
  ],
  enqueues: ['task.done'],
  flows: ['my-flow'],
}

export const handler = async ({ request }) => {
  logger.info('Processing request')

  await enqueue({
    topic: 'task.done',
    data: { result: 'success' }
  })

  return { status: 200, body: { success: true } }
}
```

</Tab>
</Tabs>

Drop this file in your `src/` folder and Motia finds it automatically. No registration, no imports, no setup.

[Learn more about Steps](/docs/concepts/steps)

---

## Event-Driven Architecture

Steps don't call each other. They **enqueue** messages to topics that other Steps consume.

This means:
- Your API can trigger a background job without waiting for it
- Steps run independently and retry on failure
- You can add new Steps without touching existing ones
- Everything is traceable from start to finish

**Example:** An API enqueues a message, a queue Step picks it up:

```typescript
// API Step enqueues
await enqueue({ topic: 'user.created', data: { email } })

// Queue Step triggers on the topic
config = {
  triggers: [
    { type: 'queue', topic: 'user.created' }
  ]
}
```

That's it. No coupling, no dependencies.

---

## Project Structure & Auto-Discovery

Motia automatically discovers Steps - no manual registration required.

### Basic Structure

<Files>
<Folder name="my-project" defaultOpen>
  <Folder name="src" defaultOpen>
    <Folder name="api">
      <File name="create-user.step.ts" />
      <File name="get-user.step.ts" />
    </Folder>
    <Folder name="queues">
      <File name="send-email.step.ts" />
      <File name="process-data_step.py" />
    </Folder>
    <Folder name="cron">
      <File name="daily-report.step.ts" />
    </Folder>
    <Folder name="streams">
      <File name="notifications.stream.ts" />
    </Folder>
  </Folder>
  <File name="config.yaml" />
  <File name=".env" />
  <File name="package.json" />
  <File name="requirements.txt" />
  <File name="tsconfig.json" />
</Folder>
</Files>

<Callout type="info">
The `src/` directory is the heart of your Motia application. All your workflow logic lives here, and Motia automatically discovers any file following the naming pattern.
</Callout>

### Auto-Discovery Rules

Motia scans the `src/` directory and automatically registers files that:

1. **Match naming pattern:**
   - TypeScript: `.step.ts`
   - JavaScript: `.step.js`
   - Python: `_step.py` (note: underscore before `step`)

2. **Export a `config` object** with Step configuration

3. **Export a `handler` function** with business logic

**No imports. No registration. Just create the file and Motia finds it.**

---

## Multi-Language Support

Every Step can be in a different language. Each language runtime runs as an independent process managed by the iii engine — Python developers do not need Node.js, and vice versa. All runtimes share the same state and communicate through the same queue infrastructure.

**Currently Supported:**
- **TypeScript** `.step.ts`
- **Python** `_step.py` (standalone `motia` Python package — no Node.js required)
- **JavaScript** `.step.js`

**Example project:**

<Files>
<Folder name="my-app" defaultOpen>
  <Folder name="src" defaultOpen>
    <File name="api-endpoint.step.ts" />
    <File name="ml-inference_step.py" />
    <File name="send-email.step.js" />
  </Folder>
</Folder>
</Files>

All three Steps work together. TypeScript API enqueues a message, Python processes with ML, JavaScript sends the result.

---

## Core Concepts

### State Management
Persistent key-value storage that works across all Steps and languages. `stateManager.set` returns `{ new_value, old_value }`.

```typescript
import { stateManager } from 'motia'

const result = await stateManager.set('users', 'user-123', { name: 'John' })
// result = { new_value: { name: 'John' }, old_value: null }
const user = await stateManager.get('users', 'user-123')
```

[Learn about State](/docs/development-guide/state-management)

### Real-Time Streams
Push live updates to connected clients (browsers, mobile apps).

```typescript
await streams.notifications.set('user-123', 'notif-1', {
  message: 'Order shipped!',
  timestamp: new Date().toISOString()
})
```

Clients receive updates instantly.

[Learn about Streams](/docs/development-guide/streams)

### Infrastructure via config.yaml
All infrastructure — queues, state storage, streams, cron scheduling, and observability — is configured through `config.yaml` modules managed by the iii engine. Swap default file-based storage with Redis or RabbitMQ without changing your application code.

[Learn about the iii Engine](/docs/concepts/iii-engine)

### Context Object
Every handler gets a context object with:

| Property | What It Does |
|----------|--------------|
| `traceId` | Request tracing |
| `trigger` | Trigger metadata |
| `is` | Type guards for trigger types |
| `getData()` | Get typed trigger data |
| `match()` | Pattern matching on trigger |

---

## Motia + iii

Motia is the application framework — you write Steps in TypeScript, Python, or JavaScript. The **iii engine** is the runtime that powers everything underneath: it manages queues, state storage, stream servers, cron scheduling, HTTP routing, and observability.

You configure iii through a `config.yaml` file that declares which modules to load and how to configure their adapters (file-based for development, Redis/RabbitMQ for production). The iii engine then manages the lifecycle of your Motia SDK processes via the `ExecModule`.

[Learn more about the iii Engine](/docs/concepts/iii-engine)

---

## Development Tool - iii Development Console

Visual interface for building and debugging flows:

- See your entire flow as a beautiful diagram
- Watch logs in real-time
- Inspect state as it changes
- View stream updates in real-time

![iii Console Dashboard](/console/dashboard.png)

[Learn about the iii development console](/docs/development-guide/observability)

---

## What's Next?

<Cards>
  <Card href="/docs/concepts/steps" title="Steps">
    Deep dive into Steps - the only primitive you need
  </Card>

  <Card href="/docs/concepts/iii-engine" title="The iii Engine">
    Understand how iii manages infrastructure through config.yaml
  </Card>

  <Card href="/docs/getting-started/quick-start" title="Quick Start">
    Build your first app in 5 minutes
  </Card>
</Cards>
