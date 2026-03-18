---
title: Multi-Trigger Steps
description: Build Steps that respond to multiple trigger types — HTTP, queue, cron, state, and stream — in a single file
---

A single Step can respond to multiple trigger types. This is useful when the same business logic should be accessible from different entry points — for example, an order processor that can be triggered manually via HTTP, automatically from a queue, and on a schedule via cron.

## Basic Example

```typescript
import type { Handlers, StepConfig } from 'motia'
import { logger, enqueue, stateManager } from 'motia'
import { z } from 'zod'

const orderSchema = z.object({ orderId: z.string(), amount: z.number() })

export const config = {
  name: 'ProcessOrder',
  description: 'Processes orders from multiple sources',
  flows: ['orders'],
  triggers: [
    { type: 'http', method: 'POST', path: '/orders/manual', bodySchema: orderSchema },
    { type: 'queue', topic: 'order.created', input: orderSchema },
    { type: 'cron', expression: '0 0 0 * * * *' },
  ],
  enqueues: ['order.processed'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input, ctx) => {
  return ctx.match({
    http: async ({ request }) => {
      await processOrder(request.body)
      return { status: 200, body: { success: true } }
    },
    queue: async (data) => {
      const payload = ctx.getData()
      await processOrder(payload)
    },
    cron: async () => {
      logger.info('Running scheduled order processing')
      const pendingOrders = await stateManager.list('pending-orders')
      for (const order of pendingOrders) {
        await processOrder(order)
      }
    },
  })
}

async function processOrder(order: any) {
  logger.info('Processing order', { orderId: order.orderId })
  await enqueue({ topic: 'order.processed', data: order })
}
```

## When to Use Multi-Trigger Steps

- **Manual + Automatic** — An HTTP trigger for manual execution and a queue trigger for automated processing
- **Scheduled + On-Demand** — A cron trigger for periodic runs and an HTTP trigger for ad-hoc execution
- **Multiple Queue Sources** — Listen to several topics when the processing logic is identical

## Handling Different Trigger Types

### Using `ctx.match()`

The `match()` method routes execution based on which trigger activated the handler:

```typescript
return ctx.match({
  http: async ({ request }) => {
    return { status: 200, body: { ok: true } }
  },
  queue: async (data) => {
    const payload = ctx.getData()
    logger.info('From queue', payload)
  },
  cron: async () => {
    logger.info('From cron')
  },
})
```

### Using `ctx.is`

For simpler branching, use the `is` type guards:

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

### Using `ctx.getData()`

When you do not care about the trigger type and just need the data payload:

```typescript
const data = ctx.getData()
logger.info('Processing', data)
```

## Best Practices

- Keep multi-trigger Steps focused on a single responsibility — the trigger variety is about *how* the Step is activated, not *what* it does
- Use `ctx.match()` when different triggers need different response handling (e.g., HTTP needs a response object, queue does not)
- Use `ctx.getData()` when the processing logic is identical regardless of trigger type
- HTTP triggers must return a response object; queue and cron triggers do not
