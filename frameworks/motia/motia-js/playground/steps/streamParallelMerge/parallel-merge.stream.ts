import { Stream, type StreamConfig } from 'motia'
import { z } from 'zod'

const parallelMergeSchema = z.object({
  startedAt: z.number(),
  totalSteps: z.number(),
  completedSteps: z.number(),
})

export const config: StreamConfig = {
  baseConfig: { storageType: 'default' },
  name: 'parallelMerge',
  schema: parallelMergeSchema,
}

export const parallelMergeStream = new Stream(config)
export type ParallelMergeStreamItem = z.infer<typeof parallelMergeSchema>
