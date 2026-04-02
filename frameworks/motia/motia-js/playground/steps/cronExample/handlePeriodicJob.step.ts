import { enqueue, type Handlers, logger, type StepConfig } from 'motia'

export const config = {
  name: 'HandlePeriodicJob',
  description: 'Handles the periodic job event',
  triggers: [
    {
      type: 'cron',
      expression: '* 1 * * * * *',
    },
  ],
  enqueues: ['periodic-job-handled'],
  flows: ['cron-example'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (_input) => {
  logger.info('Periodic job executed')

  await enqueue({
    topic: 'periodic-job-handled',
    data: { message: 'Periodic job executed' },
  })
}
