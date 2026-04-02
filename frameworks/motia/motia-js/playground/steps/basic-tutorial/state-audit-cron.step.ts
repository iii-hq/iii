import { enqueue, type Handlers, type StepConfig, stateManager, logger } from 'motia'
import type { Order } from './services/types'

export const config = {
  name: 'StateAuditJob',
  description: 'Checks the state for orders that are not complete and have a ship date in the past',
  triggers: [
    {
      type: 'cron',
      expression: '0 0/5 * * * * *', // Every 5 minutes
    },
  ],
  enqueues: ['notification'],
  flows: ['basic-tutorial'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (_input) => {
  const stateValue = await stateManager.list<Order>('orders')

  for (const item of stateValue) {
    const currentDate = new Date()
    const shipDate = new Date(item.shipDate)

    if (!item.complete && currentDate > shipDate) {
      logger.warn('Order is not complete and ship date is past', {
        orderId: item.id,
        shipDate: item.shipDate,
        complete: item.complete,
      })

      await enqueue({
        topic: 'notification',
        data: {
          email: 'test@test.com',
          templateId: 'order-audit-warning',
          templateData: {
            orderId: item.id,
            status: item.status,
            shipDate: item.shipDate,
            message: 'Order is not complete and ship date is past',
          },
        },
      })
    }
  }
}
