/**
 * Pattern: Queue Processing
 * Comparable to: BullMQ, Celery, SQS
 *
 * Enqueue work for durable, retryable async processing.
 * Standard queues process concurrently; FIFO queues preserve order.
 *
 * Retry / backoff is configured in iii-config.yaml under queue_configs:
 *   queue_configs:
 *     - name: payment
 *       max_retries: 3
 *       backoff_ms: 1000
 *       backoff_multiplier: 2
 *     - name: email
 *       fifo: true
 *       max_retries: 5
 *       backoff_ms: 500
 *
 * How-to references:
 *   - Queues: https://iii.dev/docs/how-to/use-queues
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'queue-processing',
})

// ---------------------------------------------------------------------------
// Enqueue work — standard queue (concurrent processing)
// ---------------------------------------------------------------------------
iii.registerFunction('payments::submit', async (data) => {
  const logger = new Logger()

  try {
    const result = await iii.trigger({
      function_id: 'payments::process',
      payload: {
        orderId: data.orderId,
        amount: data.amount,
        currency: data.currency || 'usd',
        method: data.paymentMethod,
      },
      action: TriggerAction.Enqueue({ queue: 'payment' }),
    })

    logger.info('Payment enqueued', {
      orderId: data.orderId,
      messageReceiptId: result.messageReceiptId,
    })

    return { status: 'queued', messageReceiptId: result.messageReceiptId }
  } catch (err) {
    logger.error('Failed to enqueue payment', { orderId: data.orderId, error: err.message })
    throw err
  }
})

// ---------------------------------------------------------------------------
// Process payment — handler that runs from the queue
// ---------------------------------------------------------------------------
iii.registerFunction('payments::process', async (data) => {
  const logger = new Logger()
  logger.info('Processing payment', { orderId: data.orderId, amount: data.amount })

  // Simulate payment gateway call
  const chargeId = `ch-${Date.now()}`

  await iii.trigger({
    function_id: 'state::set',
    payload: {
      scope: 'payments',
      key: data.orderId,
      value: {
        orderId: data.orderId,
        chargeId,
        amount: data.amount,
        currency: data.currency,
        status: 'captured',
        processed_at: new Date().toISOString(),
      },
    },
  })

  // Fire-and-forget notification
  iii.trigger({
    function_id: 'notifications::send',
    payload: { type: 'payment_captured', orderId: data.orderId, chargeId },
    action: TriggerAction.Void(),
  })

  logger.info('Payment captured', { orderId: data.orderId, chargeId })
  return { chargeId, status: 'captured' }
})

// ---------------------------------------------------------------------------
// Enqueue work — FIFO queue (ordered processing)
// FIFO queues guarantee messages are processed in the order they arrive.
// Configure fifo: true in iii-config.yaml queue_configs.
// ---------------------------------------------------------------------------
iii.registerFunction('emails::enqueue', async (data) => {
  const logger = new Logger()

  const result = await iii.trigger({
    function_id: 'emails::send',
    payload: {
      to: data.to,
      subject: data.subject,
      body: data.body,
      template: data.template,
    },
    action: TriggerAction.Enqueue({ queue: 'email' }),
  })

  logger.info('Email enqueued (FIFO)', {
    to: data.to,
    messageReceiptId: result.messageReceiptId,
  })

  return { status: 'queued', messageReceiptId: result.messageReceiptId }
})

// ---------------------------------------------------------------------------
// Process email — FIFO handler preserves send order
// ---------------------------------------------------------------------------
iii.registerFunction('emails::send', async (data) => {
  const logger = new Logger()
  logger.info('Sending email', { to: data.to, subject: data.subject })

  // Simulate email sending
  const messageId = `msg-${Date.now()}`

  await iii.trigger({
    function_id: 'state::set',
    payload: {
      scope: 'email-log',
      key: messageId,
      value: {
        messageId,
        to: data.to,
        subject: data.subject,
        status: 'sent',
        sent_at: new Date().toISOString(),
      },
    },
  })

  logger.info('Email sent', { messageId, to: data.to })
  return { messageId, status: 'sent' }
})

// ---------------------------------------------------------------------------
// Receipt capture — checking enqueue acknowledgement
// ---------------------------------------------------------------------------
iii.registerFunction('orders::place', async (data) => {
  const logger = new Logger()

  // Enqueue payment
  const paymentReceipt = await iii.trigger({
    function_id: 'payments::process',
    payload: { orderId: data.orderId, amount: data.total, currency: 'usd', method: data.method },
    action: TriggerAction.Enqueue({ queue: 'payment' }),
  })

  // Enqueue confirmation email
  const emailReceipt = await iii.trigger({
    function_id: 'emails::send',
    payload: { to: data.email, subject: 'Order confirmed', body: `Order ${data.orderId}` },
    action: TriggerAction.Enqueue({ queue: 'email' }),
  })

  logger.info('Order placed', {
    orderId: data.orderId,
    paymentReceipt: paymentReceipt.messageReceiptId,
    emailReceipt: emailReceipt.messageReceiptId,
  })

  // Store receipts for tracking
  await iii.trigger({
    function_id: 'state::set',
    payload: {
      scope: 'orders',
      key: data.orderId,
      value: {
        orderId: data.orderId,
        status: 'pending',
        paymentReceiptId: paymentReceipt.messageReceiptId,
        emailReceiptId: emailReceipt.messageReceiptId,
      },
    },
  })

  return {
    orderId: data.orderId,
    paymentReceiptId: paymentReceipt.messageReceiptId,
    emailReceiptId: emailReceipt.messageReceiptId,
  }
})

// ---------------------------------------------------------------------------
// HTTP trigger to accept orders
// ---------------------------------------------------------------------------
iii.registerTrigger({
  type: 'http',
  function_id: 'orders::place',
  config: { api_path: '/orders', http_method: 'POST' },
})
