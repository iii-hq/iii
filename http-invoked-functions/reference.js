/**
 * Pattern: HTTP-Invoked Functions
 *
 * Registers external HTTP endpoints as iii functions so the engine
 * calls them when triggered — no client-side HTTP code needed.
 * Combines with cron, state, and queue triggers for reactive integrations.
 *
 * How-to references:
 *   - HTTP-invoked functions: https://iii.dev/docs/how-to/use-functions-and-triggers#http-invoked-functions
 *   - Engine config:         https://iii.dev/docs/how-to/configure-engine
 *   - State management:      https://iii.dev/docs/how-to/manage-state
 *   - Cron:                  https://iii.dev/docs/how-to/schedule-cron-task
 *   - Queues:                https://iii.dev/docs/how-to/use-queues
 *
 * Prerequisites:
 *   - HttpFunctionsModule enabled in iii engine config
 *   - Env vars: SLACK_WEBHOOK_TOKEN, STRIPE_API_KEY, ORDER_WEBHOOK_SECRET
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'http-invoked-integrations',
})

// ---------------------------------------------------------------------------
// HTTP-invoked function: Slack webhook (bearer auth)
// ---------------------------------------------------------------------------
iii.registerFunction(
  {
    id: 'integrations::slack-notify',
    description: 'POST notification to Slack webhook',
  },
  {
    url: 'https://hooks.slack.example.com/services/incoming',
    method: 'POST',
    timeout_ms: 5000,
    headers: { 'Content-Type': 'application/json' },
    auth: {
      type: 'bearer',
      token_key: 'SLACK_WEBHOOK_TOKEN',
    },
  },
)

// ---------------------------------------------------------------------------
// HTTP-invoked function: Stripe charges (api_key auth)
// ---------------------------------------------------------------------------
iii.registerFunction(
  {
    id: 'integrations::stripe-charge',
    description: 'Create a charge via Stripe API',
  },
  {
    url: 'https://api.stripe.example.com/v1/charges',
    method: 'POST',
    timeout_ms: 10000,
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    auth: {
      type: 'api_key',
      header_name: 'Authorization',
      value_key: 'STRIPE_API_KEY',
    },
  },
)

// ---------------------------------------------------------------------------
// HTTP-invoked function: Analytics endpoint (no auth)
// ---------------------------------------------------------------------------
iii.registerFunction(
  {
    id: 'integrations::analytics-track',
    description: 'POST event to analytics service',
  },
  {
    url: 'https://analytics.internal.example.com/events',
    method: 'POST',
    timeout_ms: 3000,
  },
)

// ---------------------------------------------------------------------------
// HTTP-invoked function: Order status webhook (hmac auth)
// ---------------------------------------------------------------------------
iii.registerFunction(
  {
    id: 'integrations::order-webhook',
    description: 'POST order status change to fulfillment partner',
  },
  {
    url: 'https://fulfillment.partner.example.com/webhooks/orders',
    method: 'POST',
    timeout_ms: 5000,
    auth: {
      type: 'hmac',
      secret_key: 'ORDER_WEBHOOK_SECRET',
    },
  },
)

// ---------------------------------------------------------------------------
// Handler-based function that triggers HTTP-invoked functions
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'orders::process' }, async (data) => {
  const logger = new Logger()

  await iii.trigger({
    function_id: 'state::set',
    payload: { scope: 'orders', key: data.orderId, value: { ...data, status: 'processing' } },
  })

  // Charge payment via Stripe (await result)
  const chargeResult = await iii.trigger({
    function_id: 'integrations::stripe-charge',
    payload: { amount: data.amount, currency: 'usd', source: data.paymentToken },
  })

  logger.info('Payment charged', { orderId: data.orderId, chargeId: chargeResult.id })

  await iii.trigger({
    function_id: 'state::set',
    payload: { scope: 'orders', key: data.orderId, value: { ...data, status: 'charged' } },
  })

  // Notify Slack (fire-and-forget)
  iii.trigger({
    function_id: 'integrations::slack-notify',
    payload: { text: `Order ${data.orderId} charged $${data.amount}` },
    action: TriggerAction.Void(),
  })

  // Track in analytics (fire-and-forget)
  iii.trigger({
    function_id: 'integrations::analytics-track',
    payload: { event: 'order.charged', properties: { orderId: data.orderId, amount: data.amount } },
    action: TriggerAction.Void(),
  })

  return { orderId: data.orderId, chargeId: chargeResult.id, status: 'charged' }
})

// ---------------------------------------------------------------------------
// Trigger: state change → notify fulfillment partner via HTTP-invoked function
// ---------------------------------------------------------------------------
iii.registerTrigger({
  type: 'state',
  function_id: 'integrations::order-webhook',
  config: { scope: 'orders', key: 'status' },
})

// ---------------------------------------------------------------------------
// Trigger: scheduled analytics ping every hour
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'integrations::hourly-heartbeat' }, async () => {
  const logger = new Logger()
  const workerCount = await iii.trigger({ function_id: 'engine::workers::list', payload: {} })

  await iii.trigger({
    function_id: 'integrations::analytics-track',
    payload: {
      event: 'system.heartbeat',
      properties: { workers: workerCount.length, timestamp: new Date().toISOString() },
    },
  })

  logger.info('Hourly heartbeat sent')
})

iii.registerTrigger({
  type: 'cron',
  function_id: 'integrations::hourly-heartbeat',
  config: { expression: '0 0 * * * * *' },
})

// ---------------------------------------------------------------------------
// Trigger: enqueue Stripe charges for reliable delivery with retries
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'orders::enqueue-charge' }, async (data) => {
  const result = await iii.trigger({
    function_id: 'integrations::stripe-charge',
    payload: { amount: data.amount, currency: 'usd', source: data.paymentToken },
    action: TriggerAction.Enqueue({ queue: 'payments' }),
  })

  return { messageReceiptId: result.messageReceiptId }
})
