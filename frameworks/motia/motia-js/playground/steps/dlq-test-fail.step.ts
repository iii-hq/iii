import { type Handlers, logger, type StepConfig } from 'motia'

export const config = {
  name: 'DLQ Test Fail',
  description: 'DLQ testing - slow processing, fails unless payload has success: true',
  triggers: [
    {
      type: 'queue',
      topic: 'dlq-test',
    },
  ],
  enqueues: [],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input: any) => {
  const delay = input?.delay ?? 3000
  logger.info(`DLQ test: processing message (${delay}ms delay)...`, input)

  // Simulate slow processing
  await new Promise((resolve) => setTimeout(resolve, delay))

  if (input?.success === true) {
    logger.info('DLQ test: processed successfully', input)
    return
  }

  logger.info('DLQ test: failing message', input)
  throw new Error('Simulated failure for DLQ testing')
}
