import { ApiRouteConfig, Handlers } from '@iii-dev/motia'
import { z } from 'zod'

export const config: ApiRouteConfig = {
  type: 'api',
  name: 'DeleteTodo',
  description: 'Delete a todo item',
  flows: ['todo-app'],

  method: 'DELETE',
  path: '/todo/{todoId}',
  responseSchema: {
    200: z.object({ id: z.string() }),
    404: z.object({ error: z.string() }),
  },
  emits: [],
  virtualSubscribes: ['todo-created'],
}

export const handler: Handlers['DeleteTodo'] = async (req, { logger, streams }) => {
  const { todoId } = req.pathParams

  logger.info('Deleting todo', { todoId })

  const existingTodo = await streams.todo.get('inbox', todoId)

  if (!existingTodo) {
    logger.warn('Todo not found', { todoId })

    return {
      status: 404,
      body: { error: `Todo with id ${todoId} not found` },
    }
  }

  await streams.todo.delete('inbox', todoId)

  logger.info('Todo deleted successfully', { todoId })

  return {
    status: 200,
    body: { id: todoId },
  }
}
