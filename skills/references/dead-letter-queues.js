/**
 * Pattern: Dead Letter Queues
 * Comparable to: SQS DLQ, RabbitMQ dead-letter exchanges, BullMQ failed jobs
 *
 * When a queued function exhausts its retry budget (configured via
 * queue_configs.max_retries and backoff_ms in iii.config.yaml) the message
 * moves to the queue's dead-letter queue (DLQ). Messages in the DLQ can be
 * inspected and redriven back to the source queue via the SDK or CLI.
 *
 * How-to references:
 *   - Dead letter queues: https://iii.dev/docs/how-to/dead-letter-queues
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'dead-letter-queues',
})

// ---------------------------------------------------------------------------
// Queue configuration reference (iii.config.yaml)
//
//   queue_configs:
//     payment:
//       max_retries: 3        # after 3 failures the message goes to DLQ
//       backoff_ms: 1000      # exponential backoff base
//     email:
//       max_retries: 5
//       backoff_ms: 2000
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 1. Function that processes payments — may fail and exhaust retries
// After max_retries failures the message lands in the "payment" DLQ.
// ---------------------------------------------------------------------------
iii.registerFunction('payments::charge', async (data) => {
  const logger = new Logger()
  logger.info('Attempting payment charge', { orderId: data.order_id })

  // Simulate a transient failure (e.g. gateway timeout)
  const gatewayUp = Math.random() > 0.7
  if (!gatewayUp) {
    throw new Error('Payment gateway timeout — will be retried')
  }

  logger.info('Payment succeeded', { orderId: data.order_id })
  return { charged: true, order_id: data.order_id }
})

iii.registerTrigger({
  type: 'durable:subscriber',
  function_id: 'payments::charge',
  config: { topic: 'payment' },
})

// ---------------------------------------------------------------------------
// 2. Enqueue a payment to demonstrate the retry / DLQ flow
// ---------------------------------------------------------------------------
iii.registerFunction('orders::submit-payment', async (data) => {
  const logger = new Logger()

  const receipt = await iii.trigger({
    function_id: 'payments::charge',
    payload: { order_id: data.order_id, amount: data.amount },
    action: TriggerAction.Enqueue({ queue: 'payment' }),
  })

  logger.info('Payment enqueued', { receiptId: receipt.messageReceiptId })
  return receipt
})

iii.registerTrigger({
  type: 'http',
  function_id: 'orders::submit-payment',
  config: { api_path: '/orders/pay', http_method: 'POST' },
})

// ---------------------------------------------------------------------------
// 3. Redrive DLQ messages back to the source queue via SDK
// Calls the built-in iii::queue::redrive function. Returns the queue name
// and the count of redriven messages.
// ---------------------------------------------------------------------------
iii.registerFunction('admin::redrive-payments', async () => {
  const logger = new Logger()

  const result = await iii.trigger({
    function_id: 'iii::queue::redrive',
    payload: { queue: 'payment' },
  })

  // result shape: { queue: 'payment', redriven: 12 }
  logger.info('Redrive complete', { queue: result.queue, redriven: result.redriven })
  return result
})

iii.registerTrigger({
  type: 'http',
  function_id: 'admin::redrive-payments',
  config: { api_path: '/admin/redrive/payments', http_method: 'POST' },
})

// ---------------------------------------------------------------------------
// CLI alternative for redrive (run from terminal — iii trigger is part of the engine binary):
//   iii trigger --function-id='iii::queue::redrive' --payload='{"queue": "payment"}'
//   iii trigger --function-id='iii::queue::redrive' --payload='{"queue": "payment"}' --timeout-ms=60000
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 4. DLQ inspection pattern — check how many messages are stuck
// ---------------------------------------------------------------------------
iii.registerFunction('admin::dlq-status', async () => {
  const logger = new Logger()

  // Inspect DLQ for each configured queue
  const queues = ['payment', 'email']
  const statuses = []

  for (const queue of queues) {
    const info = await iii.trigger({
      function_id: 'iii::queue::status',
      payload: { queue },
    })

    logger.info('Queue status', { queue, dlq_count: info.dlq_count, pending: info.pending })
    statuses.push({ queue, dlq_count: info.dlq_count, pending: info.pending })
  }

  return { queues: statuses }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'admin::dlq-status',
  config: { api_path: '/admin/dlq/status', http_method: 'GET' },
})

// ---------------------------------------------------------------------------
// 5. Targeted redrive — redrive a single queue from a cron schedule
// Useful for automatically retrying failed messages every hour.
// ---------------------------------------------------------------------------
iii.registerFunction('admin::auto-redrive', async () => {
  const logger = new Logger()

  const result = await iii.trigger({
    function_id: 'iii::queue::redrive',
    payload: { queue: 'payment' },
  })

  if (result.redriven > 0) {
    logger.info('Auto-redrive recovered messages', { redriven: result.redriven })
  }

  return result
})

iii.registerTrigger({
  type: 'cron',
  function_id: 'admin::auto-redrive',
  config: { expression: '0 * * * *' }, // every hour
})
