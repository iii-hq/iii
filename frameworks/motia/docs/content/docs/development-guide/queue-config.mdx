---
title: Queue Config
description: Configure queue behavior, retries, and concurrency for Queue Steps
---

Queue config settings let you control how Queue Steps handle retries, concurrency, and backoff. Motia provides sensible defaults, so you only configure what you need.

## How It Works

Add `config` to your queue trigger:

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { type Handlers, type StepConfig } from 'motia'

export const config = {
  name: 'SendEmail',
  description: 'Send email with retry support',
  triggers: [
    {
      type: 'queue',
      topic: 'email.requested',
      config: {
        maxRetries: 5,
        visibilityTimeout: 60
      }
    },
  ],
  flows: ['notifications'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='Python'>

```python
config = {
    "name": "SendEmail",
    "description": "Send email with retry support",
    "triggers": [
        {
            "type": "queue",
            "topic": "email.requested",
            "config": {
                "maxRetries": 5,
                "visibilityTimeout": 60
            }
        }
    ],
    "flows": ["notifications"]
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
export const config = {
  name: 'SendEmail',
  description: 'Send email with retry support',
  triggers: [
    {
      type: 'queue',
      topic: 'email.requested',
      config: {
        maxRetries: 5,
        visibilityTimeout: 60
      }
    },
  ],
  flows: ['notifications']
}
```

</Tab>
</Tabs>

---

## Configuration Options

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `type` | `string` | `standard` | Queue type: `standard` or `fifo` |
| `maxRetries` | `number` | 3 | Number of retry attempts on failure |
| `visibilityTimeout` | `number` | 900 | Seconds before message becomes visible again |
| `delaySeconds` | `number` | 0 | Delay before processing (0-900 seconds) |
| `concurrency` | `number` | - | Max parallel message processing per topic |
| `backoffType` | `string` | - | Retry backoff strategy (e.g., `exponential`) |
| `backoffDelayMs` | `number` | - | Base delay in ms for retry backoff |

---

## FIFO Queues

FIFO queues guarantee exactly-once processing and maintain message order within a group.

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { type Handlers, type StepConfig } from 'motia'

export const config = {
  name: 'ProcessOrder',
  description: 'Process orders in FIFO order',
  triggers: [
    {
      type: 'queue',
      topic: 'order.created',
      config: { type: 'fifo' }
    },
  ],
  flows: ['orders'],
} as const satisfies StepConfig
```

</Tab>
<Tab value='Python'>

```python
config = {
    "name": "ProcessOrder",
    "description": "Process orders in FIFO order",
    "triggers": [
        {
            "type": "queue",
            "topic": "order.created",
            "config": {"type": "fifo"}
        }
    ],
    "flows": ["orders"]
}
```

</Tab>
<Tab value='JavaScript'>

```javascript
export const config = {
  name: 'ProcessOrder',
  description: 'Process orders in FIFO order',
  triggers: [
    {
      type: 'queue',
      topic: 'order.created',
      config: { type: 'fifo' }
    },
  ],
  flows: ['orders']
}
```

</Tab>
</Tabs>

### Message Group ID

When enqueuing to FIFO queues, pass a `messageGroupId`:

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { enqueue, type Handlers } from 'motia'

export const handler: Handlers<typeof config> = async ({ request }) => {
  const { orderId, customerId } = request.body

  await enqueue({
    topic: 'order.created',
    data: { orderId, customerId },
    messageGroupId: customerId
  })
}
```

</Tab>
<Tab value='Python'>

```python
from typing import Any
from motia import ApiRequest, ApiResponse, enqueue

async def handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
    order_id = request.body["orderId"]
    customer_id = request.body["customerId"]

    await enqueue({
        "topic": "order.created",
        "data": {"orderId": order_id, "customerId": customer_id},
        "messageGroupId": customer_id
    })

    return ApiResponse(status=202, body={"status": "queued"})
```

</Tab>
<Tab value='JavaScript'>

```javascript
import { enqueue } from 'motia'

export const handler = async ({ request }) => {
  const { orderId, customerId } = request.body

  await enqueue({
    topic: 'order.created',
    data: { orderId, customerId },
    messageGroupId: customerId
  })
}
```

</Tab>
</Tabs>

The `messageGroupId` ensures events are processed in order within that group.

---

## Default Values

If you don't specify `config` on a queue trigger, Motia uses these defaults:

```typescript
{
  type: 'standard',
  maxRetries: 3,
  visibilityTimeout: 900,
  delaySeconds: 0
}
```

---
