import { enqueue, type Handlers, logger, type StepConfig } from 'motia'
import { z } from 'zod'

export const config = {
  name: 'HelloAPI',
  description: 'Receives hello request and enqueues event for processing',
  triggers: [
    {
      type: 'http',
      path: '/hello',
      method: 'GET',
      responseSchema: {
        200: z.object({
          message: z.string(),
          status: z.string(),
          appName: z.string(),
        }),
      },
    },
  ],
  enqueues: ['process-greeting'],
  flows: ['hello-world-flow'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async () => {
  const appName = 'III App'
  const timestamp = new Date().toISOString()

  logger.info('Hello API endpoint called', { appName, timestamp })

  await enqueue({
    topic: 'process-greeting',
    data: {
      timestamp,
      appName,
      greetingPrefix: process.env.GREETING_PREFIX || 'Hello',
      requestId: Math.random().toString(36).substring(7),
    },
  })

  return {
    status: 200,
    body: {
      message: 'Hello request received! Check logs for processing.',
      status: 'processing',
      appName,
    },
  }
}
