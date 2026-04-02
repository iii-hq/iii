import { type Handlers, logger, type StepConfig, stateManager } from 'motia'
import { z } from 'zod'
import type { ParallelMergeResult } from './parallel-merge.types'

export const config = {
  name: 'parallel-merge-step-process',
  description: 'Processes a step in the parallel merge',
  triggers: [
    {
      type: 'queue',
      topic: 'pms.step.process',
      input: z.object({ traceId: z.string(), stepIndex: z.number(), waitTime: z.number().optional() }),
    },
  ],
  flows: ['parallel-merge'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (request) => {
  if (request.waitTime) {
    const waitTime = request.waitTime as number
    logger.info(`[step-process] received pms.step.process, waiting ${waitTime}ms`, { request })
    await new Promise((resolve) => setTimeout(resolve, waitTime))
  } else {
    logger.info(`[step-process] received pms.step.process, no waiting`, { request })
  }

  await stateManager.update<ParallelMergeResult>('users', request.traceId, [
    { type: 'increment', path: 'completedSteps', by: 1 },
    {
      type: 'set',
      path: `steps.${request.stepIndex}`,
      value: { msg: `Hello from Step ${request.stepIndex}`, timestamp: Date.now() },
    },
  ])

  logger.info(`[step-process] completed step ${request.stepIndex}`, { request })
}
