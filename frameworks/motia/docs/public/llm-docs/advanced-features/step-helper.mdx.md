---
title: The step() Helper
description: Use the step() helper function for ergonomic multi-trigger Step definitions
---

The `step()` helper provides a streamlined way to define multi-trigger Steps with full type safety. It wraps your config and handler together, giving you access to `ctx.getData()` and `ctx.match()` with proper type inference.

## Basic Usage

```typescript
import { http, queue, step, logger, enqueue } from 'motia'
import { z } from 'zod'

const orderSchema = z.object({
  orderId: z.string(),
  amount: z.number(),
})

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
    http: async (request) => {
      logger.info('Manual order', { body: request.body })
      await enqueue({ topic: 'notification', data: request.body })
      return { status: 200, body: { success: true } }
    },
    queue: async (queueInput) => {
      logger.info('Processing from queue', { data })
      await enqueue({ topic: 'notification', data })
    },
  })
})
```

The `step()` function returns both the `config` and `handler` exports that Motia expects, with full type inference from the config definition.

## ctx.getData()

Extracts the data payload from the input regardless of which trigger activated the handler:

- For **HTTP triggers**, returns the request body
- For **queue triggers**, returns the message data
- For **cron triggers**, returns the cron input (typically empty)
- For **state triggers**, returns the state change event
- For **stream triggers**, returns the stream event

```typescript
const data = ctx.getData()
logger.info('Processing data', data)
```

## ctx.match()

Routes execution based on which trigger type activated the handler. Each key corresponds to a trigger type:

```typescript
return ctx.match({
  http: async (request) => {
    return { status: 200, body: { ok: true } }
  },
  queue: async (data) => {
    logger.info('From queue')
  },
  cron: async () => {
    logger.info('From cron')
  },
  state: async (stateEvent) => {
    logger.info('State changed', stateEvent)
  },
  stream: async (streamEvent) => {
    logger.info('Stream event', streamEvent)
  },
  default: async (input) => {
    logger.warn('Unknown trigger type')
  },
})
```

Only include the keys for trigger types your Step actually uses. The `default` key is optional and handles any unmatched trigger types.

## Trigger Helper Functions

Use these shorthand helpers for concise trigger definitions:

```typescript
import { http, queue, cron, state, stream } from 'motia'

triggers: [
  http('POST', '/orders', {
    bodySchema: orderSchema,
    responseSchema: { 200: responseSchema },
  }),
  queue('process-order', { input: orderSchema }),
  cron('0 0 0 * * * *'),
  state((input) => input.group_id === 'orders'),
  stream('deployment'),
]
```

| Helper | Arguments | Creates |
|---|---|---|
| `http(method, path, options?)` | HTTP method, URL path, optional body/response schemas | HTTP trigger |
| `queue(topic, options?)` | Topic name, optional input schema and queue config | Queue trigger |
| `cron(expression)` | 7-field cron expression | Cron trigger |
| `state(condition?)` | Optional condition function | State trigger |
| `stream(streamName, condition?)` | Stream name, optional condition function | Stream trigger |
