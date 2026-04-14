/**
 * Pattern: Functions & Triggers
 * Comparable to: Core primitives of iii
 *
 * Demonstrates every fundamental building block: registering functions,
 * binding triggers of each built-in type (http, durable:subscriber, cron, state, subscribe),
 * cross-function invocation, fire-and-forget calls, and external HTTP-invoked
 * functions via HttpInvocationConfig.
 *
 * How-to references:
 *   - Functions & Triggers: https://iii.dev/docs/how-to/use-functions-and-triggers
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'functions-and-triggers',
})

// ---------------------------------------------------------------------------
// 1. Register a simple function
// ---------------------------------------------------------------------------
iii.registerFunction('orders::validate', async (data) => {
  const logger = new Logger()
  logger.info('Validating order', { orderId: data.order_id })

  if (!data.order_id || !data.items?.length) {
    return { valid: false, reason: 'Missing order_id or items' }
  }
  return { valid: true, order_id: data.order_id }
})

// ---------------------------------------------------------------------------
// 2. HTTP trigger — expose a function as a REST endpoint
// ---------------------------------------------------------------------------
iii.registerTrigger({
  type: 'http',
  function_id: 'orders::validate',
  config: { api_path: '/orders/validate', http_method: 'POST' },
})

// ---------------------------------------------------------------------------
// 3. Queue trigger — process items from a named queue
// ---------------------------------------------------------------------------
iii.registerFunction('orders::fulfill', async (data) => {
  const logger = new Logger()
  logger.info('Fulfilling order', { orderId: data.order_id })
  // ... fulfillment logic
  return { fulfilled: true, order_id: data.order_id }
})

iii.registerTrigger({
  type: 'durable:subscriber',
  function_id: 'orders::fulfill',
  config: { topic: 'fulfillment' },
})

// ---------------------------------------------------------------------------
// 4. Cron trigger — run a function on a schedule
// ---------------------------------------------------------------------------
iii.registerFunction('reports::daily-summary', async () => {
  const logger = new Logger()
  logger.info('Generating daily summary')
  return { generated_at: new Date().toISOString() }
})

iii.registerTrigger({
  type: 'cron',
  function_id: 'reports::daily-summary',
  config: { expression: '0 9 * * *' }, // every day at 09:00
})

// ---------------------------------------------------------------------------
// 5. State trigger — react when a state scope/key changes
// ---------------------------------------------------------------------------
iii.registerFunction('orders::on-status-change', async (data) => {
  const logger = new Logger()
  logger.info('Order status changed', { key: data.key, value: data.value })
  return { notified: true }
})

iii.registerTrigger({
  type: 'state',
  function_id: 'orders::on-status-change',
  config: { scope: 'orders' }, // fires on any key change within scope
})

// ---------------------------------------------------------------------------
// 6. Subscribe trigger — listen for pubsub messages on a topic
// ---------------------------------------------------------------------------
iii.registerFunction('notifications::on-order-complete', async (data) => {
  const logger = new Logger()
  logger.info('Order completed event received', { orderId: data.order_id })
  return { processed: true }
})

iii.registerTrigger({
  type: 'subscribe',
  function_id: 'notifications::on-order-complete',
  config: { topic: 'orders.completed' },
})

// ---------------------------------------------------------------------------
// 7. Cross-function invocation — one function calling another
// ---------------------------------------------------------------------------
iii.registerFunction('orders::create', async (data) => {
  const logger = new Logger()

  // Synchronous call — blocks until validate returns
  const validation = await iii.trigger({
    function_id: 'orders::validate',
    payload: { order_id: data.order_id, items: data.items },
  })

  if (!validation.valid) {
    return { error: validation.reason }
  }

  // Fire-and-forget — send a notification without waiting
  iii.trigger({
    function_id: 'notifications::on-order-complete',
    payload: { order_id: data.order_id },
    action: TriggerAction.Void(),
  })

  // Enqueue — durable async handoff to fulfillment
  await iii.trigger({
    function_id: 'orders::fulfill',
    payload: { order_id: data.order_id, items: data.items },
    action: TriggerAction.Enqueue({ queue: 'fulfillment' }),
  })

  return { order_id: data.order_id, status: 'accepted' }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'orders::create',
  config: { api_path: '/orders', http_method: 'POST' },
})

// ---------------------------------------------------------------------------
// 8. External HTTP-invoked function (HttpInvocationConfig)
// Wraps a third-party API as an iii function so other functions can call it
// with iii.trigger() like any internal function.
// ---------------------------------------------------------------------------
iii.registerFunction('external::payment-gateway', {
  url: 'https://api.stripe.com/v1/charges',
  method: 'POST',
  timeout_ms: 10000,
  auth: {
    type: 'bearer',
    token_key: 'STRIPE_API_KEY',
  },
})
