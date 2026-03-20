import { type EnqueueResult, Logger, TriggerAction } from 'iii-sdk'
import { useApi } from './hooks'
import { iii } from './iii'

// --- Dead Letter Queue (DLQ) demo ---
//
// This example shows how messages land in the DLQ after exhausting retries.
// The `dlq-demo` queue is configured with max_retries: 2 and backoff_ms: 500,
// so a failing job will retry twice then move to the DLQ within ~1.5 seconds.
//
// Flow:
//   POST /dlq/enqueue  →  enqueues to "dlq-demo" queue
//   Worker always throws →  engine retries (2x with exponential backoff)
//   After max_retries   →  engine moves the job to queue:dlq-demo:dlq
//
// To send a message that succeeds (for comparison), include { "succeed": true }.

useApi(
  {
    api_path: 'dlq/enqueue',
    http_method: 'POST',
    description: 'Enqueue a job to the DLQ demo queue (fails unless payload has succeed: true)',
    metadata: { tags: ['queue', 'dlq'] },
  },
  async (req, logger) => {
    logger.info('Enqueuing DLQ demo job', { body: req.body })

    const receipt = await iii.trigger<unknown, EnqueueResult>({
      function_id: 'dlq-demo::process',
      payload: req.body,
      action: TriggerAction.Enqueue({ queue: 'dlq-demo' }),
    })

    logger.info('DLQ demo job enqueued', { receiptId: receipt.messageReceiptId })

    return {
      status_code: 202,
      body: {
        receiptId: receipt.messageReceiptId,
        note: req.body?.succeed
          ? 'Job will succeed on first attempt'
          : 'Job will fail and land in DLQ after 2 retries (~1.5s)',
      },
      headers: { 'Content-Type': 'application/json' },
    }
  },
)

// --- Worker: simulates failure to demonstrate DLQ behavior ---

const dlqLogger = new Logger(undefined, 'dlq-demo::process')

iii.registerFunction(
  { id: 'dlq-demo::process', metadata: { tags: ['queue', 'dlq'] } },
  async payload => {
    const data = payload as { succeed?: boolean; message?: string }

    if (data.succeed) {
      dlqLogger.info('Job processed successfully', { payload })
      return { processed: true }
    }

    dlqLogger.warn('Job failing — will retry or move to DLQ', { payload })
    throw new Error('Simulated failure for DLQ demo')
  },
)
