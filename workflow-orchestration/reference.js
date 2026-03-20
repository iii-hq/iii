/**
 * Pattern: Workflow Orchestration & Durable Execution
 * Comparable to: Temporal, Airflow, Inngest
 *
 * Demonstrates a durable order-fulfillment pipeline with retries,
 * step tracking via state, scheduled cleanup, and DLQ handling.
 * Each step is its own function chained via named queues.
 *
 * How-to references:
 *   - Queues & retries: https://iii.dev/docs/how-to/use-queues
 *   - DLQ handling:     https://iii.dev/docs/how-to/dead-letter-queues
 *   - Cron scheduling:  https://iii.dev/docs/how-to/schedule-cron-task
 *   - State management: https://iii.dev/docs/how-to/manage-state
 *   - Streams:          https://iii.dev/docs/how-to/stream-realtime-data
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'workflow-orchestration',
})

// Queue durability (iii-config.yaml):
// queue_configs:
//   order-validate: { max_retries: 2, backoff_ms: 1000, type: standard }
//   order-payment:
//     { max_retries: 5, backoff_ms: 2000, type: fifo, message_group_field: order_id }
//   order-ship: { max_retries: 3, backoff_ms: 1000, type: standard }
// adapter:
//   class: modules::queue::BuiltinQueueAdapter
// Failed jobs are routed to the DLQ after retries are exhausted.

// ---------------------------------------------------------------------------
// Helper — update workflow state and emit a stream event
// ---------------------------------------------------------------------------
function trackStep(orderId, step, status, detail = {}) {
  iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'orders',
      key: orderId,
      ops: [
        { type: 'set', path: 'current_step', value: step },
        { type: 'set', path: 'status', value: status },
        { type: 'set', path: `steps.${step}`, value: { status, ...detail, at: new Date().toISOString() } },
      ],
    },
    action: TriggerAction.Void(),
  })

  iii.trigger({
    function_id: 'stream::send',
    payload: {
      stream_name: 'order-progress',
      group_id: orderId,
      id: `${step}-${Date.now()}`,
      event_type: 'step_update',
      data: { step, status, ...detail },
    },
    action: TriggerAction.Void(),
  })
}

// ---------------------------------------------------------------------------
// Step 1 — Validate order
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'orders::validate' }, async (data) => {
  const logger = new Logger()
  logger.info('Validating order', { orderId: data.order_id })

  trackStep(data.order_id, 'validate', 'running')

  const isValid = data.items?.length > 0 && data.total > 0
  if (!isValid) throw new Error('Invalid order: missing items or total')

  trackStep(data.order_id, 'validate', 'complete')

  await iii.trigger({
    function_id: 'orders::charge-payment',
    payload: data,
    action: TriggerAction.Enqueue({ queue: 'order-payment' }),
  })

  return { valid: true }
})

// ---------------------------------------------------------------------------
// Step 2 — Charge payment (with retries for transient failures)
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'orders::charge-payment' }, async (data) => {
  const logger = new Logger()
  logger.info('Charging payment', { orderId: data.order_id, total: data.total })

  const snapshot = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'orders', key: data.order_id },
  })
  const paymentAttempt = Number(snapshot?.attempts?.payment ?? 0) + 1
  await iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'orders',
      key: data.order_id,
      ops: [{ type: 'set', path: 'attempts.payment', value: paymentAttempt }],
    },
  })

  trackStep(data.order_id, 'payment', 'running')
  const shouldFailForDemo =
    data.force_payment_failure || paymentAttempt <= Number(data.fail_until_payment_attempt ?? 0)
  if (shouldFailForDemo) {
    trackStep(data.order_id, 'payment', 'retrying', { paymentAttempt })
    throw new Error(`Payment provider unavailable on attempt ${paymentAttempt}`)
  }

  // Simulate payment call
  const paymentResult = { transaction_id: `txn-${Date.now()}`, charged: data.total }

  trackStep(data.order_id, 'payment', 'complete', paymentResult)

  await iii.trigger({
    function_id: 'orders::ship',
    payload: { ...data, ...paymentResult },
    action: TriggerAction.Enqueue({ queue: 'order-ship' }),
  })

  return paymentResult
})

// ---------------------------------------------------------------------------
// Step 3 — Ship order
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'orders::ship' }, async (data) => {
  const logger = new Logger()
  logger.info('Shipping order', { orderId: data.order_id })

  trackStep(data.order_id, 'shipping', 'running')

  const shipment = { tracking_number: `TRACK-${Date.now()}`, carrier: 'ups' }

  trackStep(data.order_id, 'shipping', 'fulfilled', shipment)

  // Broadcast completion
  iii.trigger({
    function_id: 'publish',
    payload: { topic: 'order.fulfilled', data: { order_id: data.order_id, ...shipment } },
    action: TriggerAction.Void(),
  })

  return shipment
})

// ---------------------------------------------------------------------------
// Cron — clean up stale orders every hour
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'orders::cleanup-stale' }, async () => {
  const logger = new Logger()
  const orders = await iii.trigger({
    function_id: 'state::list',
    payload: { scope: 'orders' },
  })

  let cleaned = 0
  const ONE_DAY = 24 * 60 * 60 * 1000

  for (const order of orders) {
    const stepTime = order.steps?.[order.current_step]?.at
    if (stepTime && Date.now() - new Date(stepTime).getTime() > ONE_DAY) {
      await iii.trigger({
        function_id: 'state::update',
        payload: {
          scope: 'orders',
          key: order._key,
          ops: [{ type: 'set', path: 'status', value: 'stale' }],
        },
      })
      cleaned++
    }
  }

  logger.info('Cleaned stale orders', { cleaned })
  return { cleaned }
})

iii.registerTrigger({
  type: 'cron',
  function_id: 'orders::cleanup-stale',
  config: { expression: '0 0 * * * * *' }, // every hour
})

// ---------------------------------------------------------------------------
// HTTP — create a new order (entry point)
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'orders::create' }, async (data) => {
  const order_id = `ord-${Date.now()}`
  const force_payment_failure = Boolean(data.force_payment_failure)
  const fail_until_payment_attempt = Number(data.fail_until_payment_attempt ?? 0)

  await iii.trigger({
    function_id: 'state::set',
    payload: {
      scope: 'orders',
      key: order_id,
      value: {
        _key: order_id,
        order_id,
        items: data.items,
        total: data.total,
        force_payment_failure,
        fail_until_payment_attempt,
        status: 'created',
        current_step: 'created',
        steps: {},
        created_at: new Date().toISOString(),
      },
    },
  })

  const enqueueResult = await iii.trigger({
    function_id: 'orders::validate',
    payload: { order_id, ...data, force_payment_failure, fail_until_payment_attempt },
    action: TriggerAction.Enqueue({ queue: 'order-validate' }),
  })

  return { order_id, status: 'created', enqueue_receipt_id: enqueueResult.messageReceiptId }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'orders::create',
  config: { api_path: '/orders', http_method: 'POST' },
})
