import { enqueue, type Handlers, logger, type StepConfig } from 'motia'
import { z } from 'zod'
import { parallelMergeStream } from './parallel-merge.stream'
import { randomNumber } from './utils'

const bodySchema = z.object({
  traceId: z
    .string()
    .optional()
    .default(() => crypto.randomUUID()),
  totalSteps: z.number().optional().default(3),
  waitTime: z.boolean().optional().default(false),
  waitTimeMin: z.number().optional().default(1_000),
  waitTimeMax: z.number().optional().default(3_000),
  useEnqueue: z.boolean().optional().default(true),
})

export const config = {
  name: 'start-stream-parallel-merge',
  description: 'Initiates a parallel merge workflow using streams, triggering multiple step processing events',
  triggers: [
    {
      type: 'http',
      method: 'POST',
      path: '/api/stream-parallel-merge',
      bodySchema,
    },
  ],
  enqueues: ['spms.step.process'],
  flows: ['stream-parallel-merge'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  const body = bodySchema.parse(request.body ?? {})
  const { traceId, totalSteps, waitTimeMin, waitTimeMax } = body

  logger.info('Starting stream parallel merge', { body })

  await parallelMergeStream.set('merge-groups', traceId, {
    totalSteps,
    startedAt: Date.now(),
    completedSteps: 0,
  })

  const waitTime = () => (body.waitTime ? randomNumber(waitTimeMin, waitTimeMax) : undefined)

  if (body.useEnqueue) {
    logger.info('Using enqueue to trigger step process')

    await Promise.all(
      Array.from({ length: body.totalSteps }, (_, stepIndex) =>
        enqueue({
          topic: 'spms.step.process',
          data: { traceId, stepIndex, waitTime: waitTime() },
        }),
      ),
    )
  } else {
    logger.info('Using stream updates to trigger step process')

    await Promise.all(
      Array.from({ length: totalSteps }, () =>
        parallelMergeStream.update('merge-groups', traceId, [{ type: 'increment', path: 'completedSteps', by: 1 }]),
      ),
    )
  }

  return {
    status: 200,
    body: { message: 'Started stream parallel merge', traceId },
  }
}
