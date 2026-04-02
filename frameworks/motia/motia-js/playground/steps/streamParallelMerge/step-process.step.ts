import { type Handlers, logger, type StepConfig } from 'motia'
import { z } from 'zod'
import { parallelMergeStream } from './parallel-merge.stream'

export const config = {
  name: 'stream-parallel-merge-step-process',
  description: 'Processes individual steps in the stream parallel merge workflow',
  triggers: [
    {
      type: 'queue',
      topic: 'spms.step.process',
      input: z.object({ traceId: z.string(), stepIndex: z.number(), waitTime: z.number().optional() }),
    },
  ],
  flows: ['stream-parallel-merge'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (request) => {
  if (request.waitTime) {
    const waitTime = request.waitTime as number
    logger.info(`[stream-step-process] received spms.step.process, waiting ${waitTime}ms`)
    await new Promise((resolve) => setTimeout(resolve, waitTime))
  } else {
    logger.info(`[stream-step-process] received spms.step.process, no waiting`)
  }

  await parallelMergeStream.update('merge-groups', request.traceId, [
    { type: 'increment', path: 'completedSteps', by: 1 },
  ])

  logger.info(`[stream-step-process] completed step ${request.stepIndex}`)
}
