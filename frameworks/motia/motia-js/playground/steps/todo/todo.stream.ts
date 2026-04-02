import { logger, Stream, type StreamConfig } from 'motia'
import { z } from 'zod'
import { inboxStream } from './inbox.stream'

const todoSchema = z.object({
  id: z.string(),
  description: z.string(),
  createdAt: z.string(),
  dueDate: z.string().optional(),
  completedAt: z.string().optional(),
})

export const config: StreamConfig = {
  baseConfig: { storageType: 'default' },
  name: 'todo',
  schema: todoSchema,

  onJoin: async (subscription, _context, authContext) => {
    await inboxStream.update('watching', subscription.groupId, [{ type: 'increment', path: 'watching', by: 1 }])

    logger.info('Todo stream joined', { subscription, authContext })
    return { unauthorized: false }
  },

  onLeave: async (subscription, _context, authContext) => {
    await inboxStream.update('watching', subscription.groupId, [{ type: 'decrement', path: 'watching', by: 1 }])

    logger.info('Todo stream left', { subscription, authContext })
  },
}

export const todoStream = new Stream(config)
export type Todo = z.infer<typeof todoSchema>
