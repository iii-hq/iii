import { Logger, TriggerAction } from 'iii-sdk'
import { useApi } from './hooks'
import { iii } from './iii'

// --- Bulk email: enqueue one job per recipient from a campaign ---

useApi(
  {
    api_path: 'campaigns/launch',
    http_method: 'POST',
    description: 'Launch an email campaign — enqueue one job per recipient',
    metadata: { tags: ['queue', 'bulk-email'] },
  },
  async (req, logger) => {
    const { subject, body, recipients } = req.body as {
      subject: string
      body: string
      recipients: { email: string }[]
    }

    logger.info('Launching campaign', { subject, recipientCount: recipients.length })

    for (const recipient of recipients) {
      await iii.trigger({
        function_id: 'emails::send',
        payload: { to: recipient.email, subject, body },
        action: TriggerAction.Enqueue({ queue: 'bulk-email' }),
      })
    }

    logger.info('Campaign enqueued', { enqueued: recipients.length })

    return {
      status_code: 202,
      body: { enqueued: recipients.length },
      headers: { 'Content-Type': 'application/json' },
    }
  },
)

// --- Worker: send a single email ---

const sendLogger = new Logger(undefined, 'emails::send')

iii.registerFunction(
  { id: 'emails::send', metadata: { tags: ['queue', 'bulk-email'] } },
  async payload => {
    sendLogger.info('Sending email', { to: payload.to, subject: payload.subject })
    // Simulate SMTP delivery
    return { sent: true, to: payload.to }
  },
)
