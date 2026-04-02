import { type Handlers, logger, type StepConfig } from 'motia'
import { z } from 'zod'
import { todoStream } from './todo.stream'

export const config = {
  name: 'DeleteTodo',
  description: 'Delete a todo item',
  flows: ['todo-app'],
  triggers: [
    {
      type: 'http',
      method: 'DELETE',
      path: '/todo/:todoId',
      responseSchema: {
        200: z.object({ id: z.string() }),
        404: z.object({ error: z.string() }),
      },
    },
  ],
  enqueues: [],
  virtualSubscribes: ['todo-created'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  const { todoId } = request.pathParams || {}

  logger.info('Deleting todo', { todoId })

  const existingTodo = await todoStream.get('inbox', todoId)

  if (!existingTodo) {
    logger.warn('Todo not found', { todoId })

    return {
      status: 404,
      body: { error: `Todo with id ${todoId} not found` },
    }
  }

  await todoStream.delete('inbox', todoId)

  logger.info('Todo deleted successfully', { todoId })

  return {
    status: 200,
    body: { id: todoId },
  }
}
