import { Logger, TriggerAction } from 'iii-sdk'
import { useApi } from './hooks'
import { iii } from './iii'

// --- E-Commerce: order creation fans out to payment (FIFO) + email + analytics ---

useApi(
  {
    api_path: 'orders',
    http_method: 'POST',
    description: 'Create an order and fan out to payment, email, and analytics queues',
    metadata: { tags: ['queue', 'ecommerce'] },
  },
  async (req, logger) => {
    const orderId = `ord-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`
    const { email, total } = req.body

    logger.info('Creating order', { orderId, email, total })

    // 1. Enqueue payment processing (FIFO — ordered by orderId)
    await iii.trigger({
      function_id: 'orders::process-payment',
      payload: { orderId, amount: total, currency: 'USD' },
      action: TriggerAction.Enqueue({ queue: 'payment' }),
    })

    // 2. Enqueue confirmation email (standard queue)
    await iii.trigger({
      function_id: 'emails::confirmation',
      payload: { email, orderId },
      action: TriggerAction.Enqueue({ queue: 'email' }),
    })

    // 3. Fire-and-forget analytics tracking
    await iii.trigger({
      function_id: 'analytics::track',
      payload: { event: 'order_created', orderId },
      action: TriggerAction.Void(),
    })

    logger.info('Order created', { orderId })

    return {
      status_code: 201,
      body: { orderId },
      headers: { 'Content-Type': 'application/json' },
    }
  },
)

// --- Worker: process payment ---

const paymentLogger = new Logger(undefined, 'orders::process-payment')

iii.registerFunction(
  { id: 'orders::process-payment', metadata: { tags: ['queue', 'ecommerce'] } },
  async payload => {
    paymentLogger.info('Processing payment', { payload })
    // Simulate payment processing
    return { charged: true, orderId: payload.orderId, amount: payload.amount }
  },
)

// --- Worker: send confirmation email ---

const emailLogger = new Logger(undefined, 'emails::confirmation')

iii.registerFunction(
  { id: 'emails::confirmation', metadata: { tags: ['queue', 'ecommerce'] } },
  async payload => {
    emailLogger.info('Sending confirmation email', { payload })
    // Simulate email delivery
    return { sent: true, email: payload.email, orderId: payload.orderId }
  },
)

// --- Worker: track analytics event (fire-and-forget target) ---

const analyticsLogger = new Logger(undefined, 'analytics::track')

iii.registerFunction(
  { id: 'analytics::track', metadata: { tags: ['queue', 'ecommerce'] } },
  async payload => {
    analyticsLogger.info('Tracking event', { payload })
    return { tracked: true }
  },
)
