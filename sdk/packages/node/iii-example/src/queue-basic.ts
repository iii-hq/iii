import { type EnqueueResult, Logger, TriggerAction } from 'iii-sdk'
import { useApi } from './hooks'
import { iii } from './iii'

// --- Basic enqueue to a standard queue ---

useApi(
  {
    api_path: 'queue/enqueue',
    http_method: 'POST',
    description: 'Enqueue a job to the default standard queue',
    metadata: { tags: ['queue'] },
  },
  async (req, logger) => {
    logger.info('Enqueuing job', { body: req.body })

    const receipt = await iii.trigger<unknown, EnqueueResult>({
      function_id: 'queue::process-job',
      payload: req.body,
      action: TriggerAction.Enqueue({ queue: 'default' }),
    })

    logger.info('Job enqueued', { receiptId: receipt.messageReceiptId })

    return {
      status_code: 202,
      body: { receiptId: receipt.messageReceiptId },
      headers: { 'Content-Type': 'application/json' },
    }
  },
)

// --- Enqueue with error handling ---

useApi(
  {
    api_path: 'queue/enqueue-safe',
    http_method: 'POST',
    description: 'Enqueue a job with error handling',
    metadata: { tags: ['queue'] },
  },
  async (req, logger) => {
    logger.info('Enqueuing job (safe)', { body: req.body })

    try {
      const receipt = await iii.trigger<unknown, EnqueueResult>({
        function_id: 'queue::process-job',
        payload: req.body,
        action: TriggerAction.Enqueue({ queue: 'default' }),
      })

      logger.info('Job enqueued', { receiptId: receipt.messageReceiptId })

      return {
        status_code: 202,
        body: { receiptId: receipt.messageReceiptId },
        headers: { 'Content-Type': 'application/json' },
      }
    } catch (err) {
      logger.error('Queue rejected job', { error: String(err) })

      return {
        status_code: 503,
        body: { error: 'Failed to enqueue job' },
        headers: { 'Content-Type': 'application/json' },
      }
    }
  },
)

// --- Fire-and-forget (void) ---

useApi(
  {
    api_path: 'queue/fire-and-forget',
    http_method: 'POST',
    description: 'Fire-and-forget invocation (no queue, no response)',
    metadata: { tags: ['queue'] },
  },
  async (req, logger) => {
    logger.info('Firing and forgetting', { body: req.body })

    await iii.trigger({
      function_id: 'queue::process-job',
      payload: req.body,
      action: TriggerAction.Void(),
    })

    return {
      status_code: 202,
      body: { status: 'sent' },
      headers: { 'Content-Type': 'application/json' },
    }
  },
)

// --- Worker function that processes queued jobs ---

const jobLogger = new Logger(undefined, 'queue::process-job')

iii.registerFunction({ id: 'queue::process-job', metadata: { tags: ['queue'] } }, async payload => {
  jobLogger.info('Processing job', { payload })
  return { processed: true, payload }
})
