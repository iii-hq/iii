import { enqueue, type Handlers, logger, type StepConfig, stateManager } from 'motia'
import { z } from 'zod'
import type { ParallelMergeResult } from './parallel-merge.types'
import { randomNumber } from './utils'

const bodySchema = z.object({
  totalSteps: z.number().optional().default(3),
  waitTime: z.boolean().optional().default(false),
  waitTimeMin: z.number().optional().default(1_000),
  waitTimeMax: z.number().optional().default(3_000),
})

export const config = {
  name: 'start-parallel-merge',
  description: 'Triggered when a message is received from parallel merge',
  triggers: [
    {
      type: 'http',
      method: 'POST',
      path: '/api/parallel-merge',
      bodySchema,
    },
  ],
  enqueues: ['pms.step.process'],
  flows: ['parallel-merge'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  logger.info('Starting parallel merge')

  const body = bodySchema.parse(request.body ?? {})
  const traceId = crypto.randomUUID()

  await stateManager.set<ParallelMergeResult>('users', traceId, {
    totalSteps: body.totalSteps,
    startedAt: Date.now(),
    completedSteps: 0,
  })

  const waitTime = () => (body.waitTime ? randomNumber(body.waitTimeMin, body.waitTimeMax) : undefined)

  await Promise.all(
    Array.from({ length: body.totalSteps }, (_, stepIndex) =>
      enqueue({
        topic: 'pms.step.process',
        data: { traceId, stepIndex, waitTime: waitTime() },
      }),
    ),
  )

  return {
    status: 200,
    body: { message: 'Started parallel merge' },
  }
}
