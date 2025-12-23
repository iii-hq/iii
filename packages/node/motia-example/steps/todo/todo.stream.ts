import { StreamConfig } from '@iii-dev/motia'
import { z } from 'zod'

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

  onJoin: async (subscription, context, authContext) => {
    const inbox = (await context.streams.inbox.get('watching', subscription.groupId)) ?? { watching: 0 }
    inbox.watching++
    await context.streams.inbox.set('watching', subscription.groupId, inbox)

    context.logger.info('Todo stream joined', { subscription, authContext })
    return { unauthorized: false }
  },

  onLeave: async (subscription, context, authContext) => {
    const inbox = (await context.streams.inbox.get('watching', subscription.groupId)) ?? { watching: 1 }
    inbox.watching--
    await context.streams.inbox.set('watching', subscription.groupId, inbox)

    context.logger.info('Todo stream left', { subscription, authContext })
  },
}

export type Todo = z.infer<typeof todoSchema>
