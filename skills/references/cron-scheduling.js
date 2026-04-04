/**
 * Pattern: Cron Scheduling
 * Comparable to: node-cron, APScheduler, crontab
 *
 * Schedules recurring tasks using 7-field cron expressions:
 *   second  minute  hour  day  month  weekday  year
 *
 * Cron handlers should be fast — enqueue heavy work to a queue.
 *
 * How-to references:
 *   - Cron scheduling: https://iii.dev/docs/how-to/schedule-cron-task
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'cron-scheduling',
})

// ---------------------------------------------------------------------------
// Hourly cleanup — runs at the top of every hour
// Cron: 0 0 * * * * *  (second=0, minute=0, every hour)
// ---------------------------------------------------------------------------
iii.registerFunction('cron::hourly-cleanup', async () => {
  const logger = new Logger()
  logger.info('Hourly cleanup started')

  const expiredItems = await iii.trigger({
    function_id: 'state::list',
    payload: { scope: 'sessions' },
  })

  const now = Date.now()
  let cleaned = 0

  for (const session of expiredItems || []) {
    const age = now - new Date(session.last_active).getTime()
    if (age > 3600000) {
      // Enqueue heavy deletion work instead of doing it inline
      iii.trigger({
        function_id: 'cleanup::process-expired',
        payload: { sessionId: session.id },
        action: TriggerAction.Enqueue({ queue: 'cleanup' }),
      })
      cleaned++
    }
  }

  logger.info('Hourly cleanup enqueued', { cleaned })
  return { cleaned }
})

iii.registerTrigger({
  type: 'cron',
  function_id: 'cron::hourly-cleanup',
  config: { expression: '0 0 * * * * *' },
})

// ---------------------------------------------------------------------------
// Daily report — runs at midnight every day
// Cron: 0 0 0 * * * *  (second=0, minute=0, hour=0, every day)
// ---------------------------------------------------------------------------
iii.registerFunction('cron::daily-report', async () => {
  const logger = new Logger()
  logger.info('Daily report generation started')

  const metrics = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'daily-metrics', key: 'today' },
  })

  // Enqueue heavy report generation to a queue
  const result = await iii.trigger({
    function_id: 'reports::generate',
    payload: {
      type: 'daily-summary',
      date: new Date().toISOString().split('T')[0],
      metrics: metrics || { signups: 0, orders: 0, revenue: 0 },
    },
    action: TriggerAction.Enqueue({ queue: 'reports' }),
  })

  logger.info('Daily report enqueued', { messageReceiptId: result.messageReceiptId })

  // Reset daily counters
  await iii.trigger({
    function_id: 'state::set',
    payload: {
      scope: 'daily-metrics',
      key: 'today',
      value: { signups: 0, orders: 0, revenue: 0, reset_at: new Date().toISOString() },
    },
  })

  return { status: 'enqueued' }
})

iii.registerTrigger({
  type: 'cron',
  function_id: 'cron::daily-report',
  config: { expression: '0 0 0 * * * *' },
})

// ---------------------------------------------------------------------------
// Health check — runs every 5 minutes
// Cron: 0 */5 * * * * *  (second=0, every 5th minute)
// ---------------------------------------------------------------------------
iii.registerFunction('cron::health-check', async () => {
  const logger = new Logger()
  const timestamp = new Date().toISOString()

  // Quick check — read a known state key
  const status = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'system', key: 'health' },
  })

  const healthy = !!status

  // Persist health result
  await iii.trigger({
    function_id: 'state::set',
    payload: {
      scope: 'system',
      key: 'health',
      value: { healthy, checked_at: timestamp },
    },
  })

  if (!healthy) {
    logger.warn('Health check failed', { timestamp })

    // Enqueue alert instead of blocking the cron handler
    iii.trigger({
      function_id: 'alerts::send',
      payload: { type: 'health-check-failed', timestamp },
      action: TriggerAction.Enqueue({ queue: 'alerts' }),
    })
  }

  return { healthy, checked_at: timestamp }
})

iii.registerTrigger({
  type: 'cron',
  function_id: 'cron::health-check',
  config: { expression: '0 */5 * * * * *' },
})

// ---------------------------------------------------------------------------
// Worker for enqueued cleanup tasks
// ---------------------------------------------------------------------------
iii.registerFunction('cleanup::process-expired', async (data) => {
  const logger = new Logger()

  await iii.trigger({
    function_id: 'state::delete',
    payload: { scope: 'sessions', key: data.sessionId },
  })

  logger.info('Expired session cleaned up', { sessionId: data.sessionId })
  return { deleted: data.sessionId }
})
