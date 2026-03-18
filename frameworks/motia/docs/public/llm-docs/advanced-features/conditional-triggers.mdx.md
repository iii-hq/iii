---
title: Conditional Triggers
description: Filter which messages or requests activate a Step using condition functions on triggers
---

Triggers can include a `condition` function that determines whether the Step should execute. The condition runs before the handler — if it returns `false`, the Step is skipped entirely.

## Queue Trigger Conditions

Filter queue messages based on their content:

```typescript
import type { Handlers, StepConfig } from 'motia'
import { z } from 'zod'

const orderSchema = z.object({
  orderId: z.string(),
  amount: z.number(),
  priority: z.enum(['low', 'medium', 'high']),
})

export const config = {
  name: 'ProcessHighValueOrder',
  description: 'Only processes orders above $1000',
  triggers: [
    {
      type: 'queue',
      topic: 'order.created',
      input: orderSchema,
      condition: (input) => {
        return input.amount > 1000
      },
    },
  ],
  enqueues: ['order.premium-processed'],
  flows: ['orders'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input, { logger, enqueue }) => {
  logger.info('Processing high-value order', { orderId: input.orderId, amount: input.amount })
  await enqueue({ topic: 'order.premium-processed', data: input })
}
```

## HTTP Trigger Conditions

Filter HTTP requests before the handler runs:

```typescript
const verifiedOrderSchema = z.object({
  orderId: z.string(),
  amount: z.number(),
  user: z.object({ verified: z.boolean() }),
})

export const config = {
  name: 'VerifiedUserEndpoint',
  description: 'Only accepts requests from verified users',
  triggers: [
    {
      type: 'http',
      method: 'POST',
      path: '/orders/manual',
      bodySchema: verifiedOrderSchema,
      condition: (input) => {
        return input.body.user?.verified === true
      },
    },
  ],
  enqueues: [],
  flows: ['orders'],
} as const satisfies StepConfig
```

## Multiple Triggers with Different Conditions

Each trigger in a multi-trigger Step can have its own condition:

```typescript
export const config = {
  name: 'SmartProcessor',
  triggers: [
    {
      type: 'queue',
      topic: 'task.created',
      input: taskSchema,
      condition: (input) => input.type === 'automated',
    },
    {
      type: 'http',
      method: 'POST',
      path: '/tasks/manual',
      bodySchema: taskSchema,
      condition: (input) => input.body.approved === true,
    },
  ],
  enqueues: ['task.processed'],
  flows: ['tasks'],
} as const satisfies StepConfig
```

## Use Cases

| Pattern | Description |
|---|---|
| **Amount thresholds** | Only process orders above a certain value |
| **Priority routing** | Route high-priority items to fast-track processing |
| **Feature flags** | Enable/disable Steps based on input flags |
| **Content filtering** | Skip messages that do not match expected criteria |
| **User verification** | Only accept requests from verified or authorized users |

## Condition Function Signature

```typescript
condition: (input: TriggerInput, ctx: { trigger: TriggerInfo }) => boolean
```

- **`input`** — The trigger input (request body for HTTP, message data for queue)
- **`ctx`** — Context with trigger information
- **Returns** `true` to execute the handler, `false` to skip
