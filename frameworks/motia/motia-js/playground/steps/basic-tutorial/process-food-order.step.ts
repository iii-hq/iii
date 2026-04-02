import { enqueue, http, logger, queue, stateManager, step } from 'motia'
import { z } from 'zod'
import { petStoreService } from './services/pet-store'

const orderSchema = z.object({
  email: z.string(),
  quantity: z.number(),
  petId: z.string(),
})

export const stepConfig = {
  name: 'ProcessFoodOrder',
  description: 'basic-tutorial event step, demonstrates how to consume an event from a topic and persist data in state',
  flows: ['basic-tutorial'],
  triggers: [
    queue('process-food-order', { input: orderSchema }),
    http('POST', '/process-food-order', { bodySchema: orderSchema }),
  ],
  enqueues: ['notification'],
}

export const { config, handler } = step(stepConfig, async (_input, ctx) => {
  const data = ctx.getData()

  logger.info('Step 02 - Process food order', {
    input: data,
    traceId: ctx.traceId,
    triggerType: ctx.trigger.type,
  })

  const order = await petStoreService.createOrder({
    ...data,
    shipDate: new Date().toISOString(),
    status: 'placed',
  })

  logger.info('Order created', { order })

  await stateManager.set('orders', order.id, order)

  await enqueue({
    topic: 'notification',
    data: {
      email: data.email,
      templateId: 'new-order',
      templateData: {
        status: order.status,
        shipDate: order.shipDate,
        id: order.id,
        petId: order.petId,
        quantity: order.quantity,
      },
    },
  })

  return ctx.match({
    http: async () => ({
      status: 200,
      headers: { 'content-type': 'application/json' },
      body: { success: true, order },
    }),
  })
})
