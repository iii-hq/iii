import {
  type ApiRequest,
  enqueue,
  type Handlers,
  logger,
  type StepConfig,
  stateManager,
  type TriggerCondition,
} from 'motia'
import { z } from 'zod'

const isHighValue: TriggerCondition<{ amount: number; description: string }> = (input, _ctx) => {
  if (!input || typeof input !== 'object') return false
  return (input as { amount: number }).amount > 1000
}

const isVerifiedUser: TriggerCondition<{ user: { verified: boolean }; amount: number; description: string }> = (
  input,
  ctx,
) => {
  if (ctx.trigger.type !== 'http' || !input) return false
  const apiInput = input as ApiRequest<{ user: { verified: boolean }; amount: number; description: string }>
  return apiInput.body.user.verified === true
}

export const config = {
  name: 'MultiTriggerExample',
  description: 'Demonstrates multi-trigger with conditions - processes orders via event, API, or cron',
  flows: ['multi-trigger-demo'],
  triggers: [
    {
      type: 'queue',
      topic: 'order.created',
      input: z.object({
        amount: z.number(),
        description: z.string(),
      }),
      condition: (input, ctx) => {
        return isHighValue(input, ctx)
      },
    },
    {
      type: 'http',
      method: 'POST',
      path: '/orders/manual',
      bodySchema: z.object({
        user: z.object({
          verified: z.boolean(),
        }),
        amount: z.number(),
        description: z.string(),
      }),
      responseSchema: {
        200: z.object({
          message: z.string(),
          orderId: z.string(),
          processedBy: z.string(),
        }),
        403: z.object({
          error: z.string(),
        }),
      },
      condition: isVerifiedUser,
    },
    {
      type: 'cron',
      expression: '* * * * *', //
    },
  ],
  enqueues: ['order.processed'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (_, ctx): Promise<any> => {
  logger.info('Processing order')

  const orderId = `order-${Date.now()}-${Math.random().toString(36).substring(7)}`

  return ctx.match({
    http: async ({ request }) => {
      const body = request.body

      logger.info('Processing manual order via API', {
        amount: body.amount,
        user: body.user,
      })

      await stateManager.set('orders', orderId, {
        id: orderId,
        amount: body.amount,
        description: body.description,
        source: 'manual-api',
        createdAt: new Date().toISOString(),
      })

      await enqueue({
        topic: 'order.processed',
        data: {
          orderId,
          amount: body.amount,
          source: 'manual-api',
        },
      })

      return {
        status: 200,
        body: {
          message: 'Order processed successfully',
          orderId,
          processedBy: 'manual-api',
        },
      }
    },

    queue: async (queueInput) => {
      const { amount, description } = queueInput

      logger.info('Processing order from queue', {
        amount,
        description,
      })

      await stateManager.set('orders', orderId, {
        id: orderId,
        amount,
        description,
        source: 'event',
        createdAt: new Date().toISOString(),
      })

      await enqueue({
        topic: 'order.processed',
        data: {
          orderId,
          amount,
          source: 'event',
        },
      })
    },

    cron: async () => {
      logger.info('Processing scheduled order batch')

      const pendingOrders = await stateManager.list<{ id: string; amount: number }>('pending-orders')

      for (const order of pendingOrders) {
        await enqueue({
          topic: 'order.processed',
          data: {
            orderId: order.id,
            amount: order.amount,
            source: 'cron-batch',
          },
        })
      }

      logger.info('Scheduled batch processing complete', {
        processedCount: pendingOrders.length,
      })
    },
  })
}
