---
title: Flows
description: Group multiple steps to be visible in diagrams in the iii development console
---

Flows group related Steps together so you can see them as connected workflows in the [iii development console](https://iii.dev/docs). Add `flows` to your Step config - it's an array of flow names.

## How It Works

Add `flows` to any Step config. Each string is a flow name.

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { type Handlers, type StepConfig } from 'motia'

export const config = {
  name: 'CreateResource',
  description: 'Create a new resource',
  triggers: [
    { type: 'http', path: '/resources', method: 'POST' },
  ],
  enqueues: [],
  flows: ['resource-management'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='Python'>

```python
config = {
    "name": "CreateResource",
    "description": "Create a new resource",
    "triggers": [
        {"type": "http", "path": "/resources", "method": "POST"}
    ],
    "enqueues": [],
    "flows": ["resource-management"]
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
export const config = {
  name: 'CreateResource',
  description: 'Create a new resource',
  triggers: [
    { type: 'http', path: '/resources', method: 'POST' },
  ],
  enqueues: [],
  flows: ['resource-management']
}
```

</Tab>
</Tabs>

---

## Example

Two Steps connected through a single flow definition.

**HTTP Step - Create resource:**

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript title="src/create-resource.step.ts"
import { type Handlers, type StepConfig, enqueue, logger } from 'motia'

export const config = {
  name: 'CreateResource',
  description: 'Create a new resource and trigger email',
  triggers: [
    { type: 'http', path: '/resources', method: 'POST' },
  ],
  enqueues: ['send-email'],
  flows: ['resource-management'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  logger.info('Creating resource', { title: request.body.title })

  await enqueue({
    topic: 'send-email',
    data: { email: request.body.email }
  })

  return { status: 201, body: { id: '123' } }
}
```

</Tab>
<Tab value='Python'>

```python title="src/create_resource_step.py"
from typing import Any
from motia import ApiRequest, ApiResponse, http, logger, enqueue

config = {
    "name": "CreateResource",
    "description": "Create a new resource and trigger email",
    "triggers": [
        http("POST", "/resources"),
    ],
    "enqueues": ["send-email"],
    "flows": ["resource-management"]
}

async def handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
    logger.info("Creating resource", {"title": request.body["title"]})

    await enqueue({
        "topic": "send-email",
        "data": {"email": request.body["email"]}
    })

    return ApiResponse(status=201, body={"id": "123"})
```

</Tab>
<Tab value='JavaScript'>

```javascript title="src/create-resource.step.js"
import { enqueue, logger } from 'motia'

export const config = {
  name: 'CreateResource',
  description: 'Create a new resource and trigger email',
  triggers: [
    { type: 'http', path: '/resources', method: 'POST' },
  ],
  enqueues: ['send-email'],
  flows: ['resource-management']
}

export const handler = async ({ request }) => {
  logger.info('Creating resource', { title: request.body.title })

  await enqueue({
    topic: 'send-email',
    data: { email: request.body.email }
  })

  return { status: 201, body: { id: '123' } }
}
```

</Tab>
</Tabs>

**Queue Step - Send email:**

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript title="src/send-email.step.ts"
import { type Handlers, type StepConfig, logger } from 'motia'

export const config = {
  name: 'SendEmail',
  description: 'Send an email notification',
  triggers: [
    { type: 'queue', topic: 'send-email' },
  ],
  enqueues: [],
  flows: ['resource-management'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input) => {
  logger.info('Sending email', { email: input.email })
}
```

</Tab>
<Tab value='Python'>

```python title="src/send_email_step.py"
from motia import logger

config = {
    "name": "SendEmail",
    "description": "Send an email notification",
    "triggers": [
        {"type": "queue", "topic": "send-email"}
    ],
    "enqueues": [],
    "flows": ["resource-management"]
}

async def handler(input):
    logger.info("Sending email", {"email": input["email"]})
```

</Tab>
<Tab value='JavaScript'>

```javascript title="src/send-email.step.js"
import { logger } from 'motia'

export const config = {
  name: 'SendEmail',
  description: 'Send an email notification',
  triggers: [
    { type: 'queue', topic: 'send-email' },
  ],
  enqueues: [],
  flows: ['resource-management']
}

export const handler = async (input) => {
  logger.info('Sending email', { email: input.email })
}
```

</Tab>
</Tabs>

Both Steps have `flows: ['resource-management']`. In the iii development console, they appear connected.

---

## Multiple Flows

A Step can belong to multiple flows.

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { type Handlers, type StepConfig } from 'motia'

export const config = {
  name: 'SendEmail',
  description: 'Send an email notification',
  triggers: [
    { type: 'queue', topic: 'send-email' },
  ],
  enqueues: [],
  flows: ['resource-management', 'user-onboarding'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='Python'>

```python
config = {
    "name": "SendEmail",
    "description": "Send an email notification",
    "triggers": [
        {"type": "queue", "topic": "send-email"}
    ],
    "enqueues": [],
    "flows": ["resource-management", "user-onboarding"]
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
export const config = {
  name: 'SendEmail',
  description: 'Send an email notification',
  triggers: [
    { type: 'queue', topic: 'send-email' },
  ],
  enqueues: [],
  flows: ['resource-management', 'user-onboarding']
}
```

</Tab>
</Tabs>

This Step appears in both flows in the iii development console.

## Steps Without Flows

Steps work fine without flows.

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { type Handlers, type StepConfig } from 'motia'

export const config = {
  name: 'HealthCheck',
  description: 'Health check endpoint',
  triggers: [
    { type: 'http', path: '/health', method: 'GET' },
  ],
  enqueues: [],
} as const satisfies StepConfig
```

</Tab>
<Tab value='Python'>

```python
config = {
    "name": "HealthCheck",
    "description": "Health check endpoint",
    "triggers": [
        {"type": "http", "path": "/health", "method": "GET"}
    ],
    "enqueues": []
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
export const config = {
  name: 'HealthCheck',
  description: 'Health check endpoint',
  triggers: [
    { type: 'http', path: '/health', method: 'GET' },
  ],
  enqueues: [],
}
```

</Tab>
</Tabs>

---

## Flows in the iii Development Console

The iii development console has a dropdown to filter by flow. Select a flow to see only the Steps that belong to it.

![Flow view in the iii Console](/console/flow-view.png)

### Virtual Connections

Use `virtualEnqueues` and `virtualSubscribes` for visualization without actual events:

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { type StepConfig } from 'motia'

export const config = {
  name: 'CreateResource',
  description: 'Create a resource requiring approval',
  triggers: [
    { type: 'http', path: '/resources', method: 'POST' },
  ],
  enqueues: [],
  virtualEnqueues: ['approval.required'],
  flows: ['resource-management'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='Python'>

```python
config = {
    "name": "CreateResource",
    "description": "Create a resource requiring approval",
    "triggers": [
        {"type": "http", "path": "/resources", "method": "POST"}
    ],
    "enqueues": [],
    "virtualEnqueues": ["approval.required"],
    "flows": ["resource-management"]
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
export const config = {
  name: 'CreateResource',
  description: 'Create a resource requiring approval',
  triggers: [
    { type: 'http', path: '/resources', method: 'POST' },
  ],
  enqueues: [],
  virtualEnqueues: ['approval.required'],
  flows: ['resource-management']
}
```

</Tab>
</Tabs>

Virtual connections show as gray/dashed lines in the iii development console. Real connections (from `enqueues` and queue triggers) show as dark solid lines.

### Labels

Add labels to connections in the iii development console:

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { type StepConfig } from 'motia'

export const config = {
  name: 'SendEmail',
  description: 'Send email notifications',
  triggers: [
    { type: 'http', path: '/send', method: 'POST' },
  ],
  enqueues: [],
  virtualEnqueues: [
    { topic: 'email-sent', label: 'Email delivered' },
    { topic: 'email-failed', label: 'Failed to send', conditional: true },
  ],
  flows: ['notifications'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='Python'>

```python
config = {
    "name": "SendEmail",
    "description": "Send email notifications",
    "triggers": [
        {"type": "http", "path": "/send", "method": "POST"}
    ],
    "enqueues": [],
    "virtualEnqueues": [
        {"topic": "email-sent", "label": "Email delivered"},
        {"topic": "email-failed", "label": "Failed to send", "conditional": True}
    ],
    "flows": ["notifications"]
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
export const config = {
  name: 'SendEmail',
  description: 'Send email notifications',
  triggers: [
    { type: 'http', path: '/send', method: 'POST' },
  ],
  enqueues: [],
  virtualEnqueues: [
    { topic: 'email-sent', label: 'Email delivered' },
    { topic: 'email-failed', label: 'Failed to send', conditional: true }
  ],
  flows: ['notifications']
}
```

</Tab>
</Tabs>

### NOOP Steps

NOOP Steps don't run code. They're for visualization only:

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { NoopConfig } from 'motia'

export const config: NoopConfig = {
  type: 'noop',
  name: 'ApprovalGate',
  virtualEnqueues: ['approved'],
  virtualSubscribes: ['approval.required'],
  flows: ['resource-management']
}
```

</Tab>
<Tab value='Python'>

```python
config = {
    "type": "noop",
    "name": "ApprovalGate",
    "virtualEnqueues": ["approved"],
    "virtualSubscribes": ["approval.required"],
    "flows": ["resource-management"]
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
export const config = {
  type: 'noop',
  name: 'ApprovalGate',
  virtualEnqueues: ['approved'],
  virtualSubscribes: ['approval.required'],
  flows: ['resource-management']
}
```

</Tab>
</Tabs>

[Learn more about Steps and Triggers](/docs/concepts/steps)

---
