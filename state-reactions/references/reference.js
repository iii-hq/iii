/**
 * Pattern: State Reactions
 * Comparable to: Firebase onSnapshot, Convex mutations
 *
 * Register functions that fire automatically when state changes
 * in a given scope. Optionally filter with a condition function
 * that returns a boolean.
 *
 * How-to references:
 *   - State reactions: https://iii.dev/docs/how-to/react-to-state-changes
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'state-reactions',
})

// ---------------------------------------------------------------------------
// Basic state reaction — fires on ANY change in the 'orders' scope
// The handler receives: { new_value, old_value, key, event_type }
//   event_type: 'set' | 'update' | 'delete'
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'reactions::order-audit-log' }, async (event) => {
  const logger = new Logger()
  const { new_value, old_value, key, event_type } = event

  const action = !old_value ? 'created' : !new_value ? 'deleted' : 'updated'
  logger.info('Order changed', { key, action, event_type })

  // Persist audit entry
  const auditId = `audit-${Date.now()}`
  await iii.trigger({
    function_id: 'state::set',
    payload: {
      scope: 'order-audit',
      key: auditId,
      value: {
        auditId,
        orderKey: key,
        action,
        event_type,
        before: old_value,
        after: new_value,
        timestamp: new Date().toISOString(),
      },
    },
  })

  return { auditId, action }
})

iii.registerTrigger({
  type: 'state',
  function_id: 'reactions::order-audit-log',
  config: { scope: 'orders' },
})

// ---------------------------------------------------------------------------
// Conditional reaction — only fires when condition function returns true
// The condition function receives the same event and must return a boolean.
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'reactions::high-value-alert-condition' }, async (event) => {
  const { new_value } = event

  // Only react when an order total exceeds $1000
  return new_value && new_value.total > 1000
})

iii.registerFunction({ id: 'reactions::high-value-alert' }, async (event) => {
  const logger = new Logger()
  const { new_value, key } = event

  logger.info('High-value order detected', { key, total: new_value.total })

  // Enqueue alert for reliable delivery
  iii.trigger({
    function_id: 'alerts::notify-manager',
    payload: {
      type: 'high-value-order',
      orderId: key,
      total: new_value.total,
      customer: new_value.customer,
    },
    action: TriggerAction.Enqueue({ queue: 'alerts' }),
  })

  return { alerted: true, orderId: key }
})

iii.registerTrigger({
  type: 'state',
  function_id: 'reactions::high-value-alert',
  config: {
    scope: 'orders',
    condition_function_id: 'reactions::high-value-alert-condition',
  },
})

// ---------------------------------------------------------------------------
// Multiple independent reactions to the same scope
// Each trigger registers a separate function on the same scope.
// All registered reactions fire independently on every matching change.
// ---------------------------------------------------------------------------

// Reaction 1: Update aggregate metrics
iii.registerFunction({ id: 'reactions::order-metrics' }, async (event) => {
  const { new_value, old_value } = event

  const ops = []

  if (new_value && !old_value) {
    ops.push({ type: 'increment', path: 'total_orders', by: 1 })
    ops.push({ type: 'increment', path: 'total_revenue', by: new_value.total || 0 })
  }

  if (!new_value && old_value) {
    ops.push({ type: 'increment', path: 'total_orders', by: -1 })
    ops.push({ type: 'increment', path: 'total_revenue', by: -(old_value.total || 0) })
  }

  if (ops.length > 0) {
    await iii.trigger({
      function_id: 'state::update',
      payload: { scope: 'order-metrics', key: 'global', ops },
    })
  }
})

iii.registerTrigger({
  type: 'state',
  function_id: 'reactions::order-metrics',
  config: { scope: 'orders' },
})

// Reaction 2: Push live update to connected clients
iii.registerFunction({ id: 'reactions::order-live-feed' }, async (event) => {
  const { new_value, old_value, key } = event
  const action = !old_value ? 'created' : !new_value ? 'deleted' : 'updated'

  iii.trigger({
    function_id: 'stream::send',
    payload: {
      stream_name: 'orders-live',
      group_id: 'dashboard',
      id: `evt-${Date.now()}`,
      event_type: 'order_changed',
      data: { action, key, order: new_value },
    },
    action: TriggerAction.Void(),
  })
})

iii.registerTrigger({
  type: 'state',
  function_id: 'reactions::order-live-feed',
  config: { scope: 'orders' },
})
