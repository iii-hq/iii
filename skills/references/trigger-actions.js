/**
 * Pattern: Trigger Actions (Invocation Modes)
 * Comparable to: Synchronous calls, async queues, fire-and-forget messaging
 *
 * Every iii.trigger() call can specify an invocation mode via the `action`
 * parameter. There are exactly three modes:
 *   1. Synchronous (default) — blocks until the target returns a result.
 *   2. Fire-and-forget (TriggerAction.Void()) — returns null immediately.
 *   3. Enqueue (TriggerAction.Enqueue({ queue })) — durably enqueues and
 *      returns { messageReceiptId }.
 *
 * This file shows each mode in isolation and then combines all three in a
 * realistic checkout workflow.
 *
 * How-to references:
 *   - Trigger actions: https://iii.dev/docs/how-to/trigger-actions
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'trigger-actions',
})

// ---------------------------------------------------------------------------
// Helper functions used by the examples below
// ---------------------------------------------------------------------------
iii.registerFunction('checkout::validate-cart', async (data) => {
  const logger = new Logger()
  logger.info('Validating cart', { cartId: data.cart_id })

  if (!data.items?.length) {
    return { valid: false, reason: 'Cart is empty' }
  }

  const total = data.items.reduce((sum, i) => sum + i.price * i.qty, 0)
  return { valid: true, cart_id: data.cart_id, total }
})

iii.registerFunction('checkout::charge-payment', async (data) => {
  const logger = new Logger()
  logger.info('Charging payment', { cart_id: data.cart_id, total: data.total })
  // Simulate payment processing
  return { charged: true, transaction_id: `txn_${Date.now()}` }
})

iii.registerFunction('checkout::send-confirmation', async (data) => {
  const logger = new Logger()
  logger.info('Sending order confirmation email', { email: data.email })
  return { sent: true }
})

// ---------------------------------------------------------------------------
// Mode 1 — Synchronous (default)
// Blocks until the target function returns. The result is the function's
// return value. Use this when the caller needs the result to continue.
// ---------------------------------------------------------------------------
iii.registerFunction('examples::sync-call', async (data) => {
  const logger = new Logger()

  // No action parameter — defaults to synchronous
  const result = await iii.trigger({
    function_id: 'checkout::validate-cart',
    payload: { cart_id: data.cart_id, items: data.items },
  })

  logger.info('Sync result received', { valid: result.valid, total: result.total })
  return result
})

// ---------------------------------------------------------------------------
// Mode 2 — Fire-and-forget (TriggerAction.Void())
// Returns null immediately. The target function runs asynchronously and its
// return value is discarded. Use for side-effects like logging, notifications,
// or analytics where the caller does not need to wait.
// ---------------------------------------------------------------------------
iii.registerFunction('examples::void-call', async (data) => {
  const logger = new Logger()

  // TriggerAction.Void() — returns null, does not wait
  iii.trigger({
    function_id: 'checkout::send-confirmation',
    payload: { email: data.email, order_id: data.order_id },
    action: TriggerAction.Void(),
  })

  logger.info('Confirmation dispatched (fire-and-forget)')
  return { dispatched: true }
})

// ---------------------------------------------------------------------------
// Mode 3 — Enqueue (TriggerAction.Enqueue({ queue }))
// Durably enqueues the payload onto a named queue. Returns immediately with
// { messageReceiptId }. The target function processes the message when a
// worker picks it up. Use for work that must survive crashes and be retried.
// ---------------------------------------------------------------------------
iii.registerFunction('examples::enqueue-call', async (data) => {
  const logger = new Logger()

  const receipt = await iii.trigger({
    function_id: 'checkout::charge-payment',
    payload: { cart_id: data.cart_id, total: data.total },
    action: TriggerAction.Enqueue({ queue: 'payments' }),
  })

  logger.info('Payment enqueued', { messageReceiptId: receipt.messageReceiptId })
  return receipt
})

// ---------------------------------------------------------------------------
// Realistic workflow — Checkout combining all three modes
//   1. Validate cart  (sync)    — need the result to decide whether to proceed
//   2. Charge payment (enqueue) — durable, retryable, must not be lost
//   3. Send email     (void)    — best-effort notification, don't block
// ---------------------------------------------------------------------------
iii.registerFunction('checkout::process', async (data) => {
  const logger = new Logger()

  // Step 1: synchronous validation — we need the total to charge
  const validation = await iii.trigger({
    function_id: 'checkout::validate-cart',
    payload: { cart_id: data.cart_id, items: data.items },
  })

  if (!validation.valid) {
    return { error: validation.reason }
  }

  // Step 2: enqueue payment — durable async, survives crashes
  const receipt = await iii.trigger({
    function_id: 'checkout::charge-payment',
    payload: { cart_id: data.cart_id, total: validation.total },
    action: TriggerAction.Enqueue({ queue: 'payments' }),
  })

  logger.info('Payment queued', { receiptId: receipt.messageReceiptId })

  // Step 3: fire-and-forget email — don't block the checkout response
  iii.trigger({
    function_id: 'checkout::send-confirmation',
    payload: { email: data.email, order_id: data.cart_id },
    action: TriggerAction.Void(),
  })

  return {
    status: 'accepted',
    cart_id: data.cart_id,
    total: validation.total,
    payment_receipt: receipt.messageReceiptId,
  }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'checkout::process',
  config: { api_path: '/checkout', http_method: 'POST' },
})
