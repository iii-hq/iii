/**
 * Pattern: Trigger Conditions
 * Comparable to: Event filters, guard clauses, conditional routing
 *
 * A trigger condition is a regular function that returns a boolean. When
 * attached to a trigger via condition_function_id, the engine calls the
 * condition first — if it returns true the handler runs, otherwise the
 * event is silently skipped. The condition receives the same event data
 * as the handler.
 *
 * How-to references:
 *   - Trigger conditions: https://iii.dev/docs/how-to/use-trigger-conditions
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'trigger-conditions',
})

// ---------------------------------------------------------------------------
// Example 1 — State trigger with a high-value order condition
// Only fires the handler when the order total exceeds $500.
// ---------------------------------------------------------------------------

// Condition function — returns true/false
iii.registerFunction({ id: 'conditions::is-high-value' }, async (data) => {
  // data is the same event the handler would receive
  return data.value?.total > 500
})

// Handler function — only runs when the condition passes
iii.registerFunction({ id: 'orders::flag-high-value' }, async (data) => {
  const logger = new Logger()
  logger.info('High-value order detected', { key: data.key, total: data.value.total })

  await iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'orders',
      key: data.key,
      ops: [{ type: 'set', path: 'flagged', value: true }],
    },
  })

  return { flagged: true, order_id: data.key }
})

// Bind the trigger with condition_function_id
iii.registerTrigger({
  type: 'state',
  function_id: 'orders::flag-high-value',
  config: {
    scope: 'orders',
    condition_function_id: 'conditions::is-high-value',
  },
})

// ---------------------------------------------------------------------------
// Example 2 — HTTP trigger with request validation condition
// Rejects requests missing a required API key header.
// ---------------------------------------------------------------------------

iii.registerFunction({ id: 'conditions::has-api-key' }, async (data) => {
  const apiKey = data.headers?.['x-api-key']
  return typeof apiKey === 'string' && apiKey.length > 0
})

iii.registerFunction({ id: 'api::protected-endpoint' }, async (data) => {
  const logger = new Logger()
  logger.info('Authenticated request', { path: data.path })
  return { message: 'Access granted', user: data.headers['x-api-key'] }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'api::protected-endpoint',
  config: {
    api_path: '/api/protected',
    http_method: 'GET',
    condition_function_id: 'conditions::has-api-key',
  },
})

// ---------------------------------------------------------------------------
// Example 3 — Queue trigger with event type filter condition
// Only processes messages whose `event_type` is "order.placed".
// ---------------------------------------------------------------------------

iii.registerFunction({ id: 'conditions::is-order-placed' }, async (data) => {
  return data.event_type === 'order.placed'
})

iii.registerFunction({ id: 'orders::on-placed' }, async (data) => {
  const logger = new Logger()
  logger.info('Processing order.placed event', { orderId: data.order_id })

  // Kick off fulfillment
  await iii.trigger({
    function_id: 'orders::fulfill',
    payload: { order_id: data.order_id },
    action: TriggerAction.Enqueue({ queue: 'fulfillment' }),
  })

  return { processed: true, order_id: data.order_id }
})

iii.registerFunction({ id: 'orders::fulfill' }, async (data) => {
  const logger = new Logger()
  logger.info('Fulfilling order', { orderId: data.order_id })
  return { fulfilled: true }
})

iii.registerTrigger({
  type: 'queue',
  function_id: 'orders::on-placed',
  config: {
    queue: 'order-events',
    condition_function_id: 'conditions::is-order-placed',
  },
})

// ---------------------------------------------------------------------------
// Example 4 — Condition with shared data
// The condition and handler receive identical event data, so a condition can
// enrich or validate any field the handler will use.
// ---------------------------------------------------------------------------

iii.registerFunction({ id: 'conditions::is-weekday' }, async (data) => {
  const day = new Date().getDay()
  return day >= 1 && day <= 5 // Monday–Friday
})

iii.registerFunction({ id: 'reports::weekday-digest' }, async () => {
  const logger = new Logger()
  logger.info('Running weekday digest')
  return { generated: true }
})

iii.registerTrigger({
  type: 'cron',
  function_id: 'reports::weekday-digest',
  config: {
    cron: '0 8 * * *', // runs daily at 08:00 but condition limits to weekdays
    condition_function_id: 'conditions::is-weekday',
  },
})
