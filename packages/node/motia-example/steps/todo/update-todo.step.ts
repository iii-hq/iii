import { ApiRouteConfig, Handlers } from 'motia'
import { z } from 'zod'
import { todoSchema } from './create-todo.step'

export const config: ApiRouteConfig = {
  type: 'api',
  name: 'UpdateTodo',
  description: 'Update an existing todo item',
  flows: ['todo-app'],

  method: 'PUT',
  path: '/todo/:todoId',
  bodySchema: z.object({
    description: z.string().optional(),
    dueDate: z.string().optional(),
    checked: z.boolean().optional(),
  }),
  responseSchema: {
    200: todoSchema,
    404: z.object({ error: z.string() }),
  },
  emits: [],
  virtualSubscribes: ['todo-created'],
}

export const handler: Handlers['UpdateTodo'] = async (req, { logger, streams }) => {
  const { todoId } = req.pathParams
  logger.info('Updating todo', { todoId, body: req.body })

  const existingTodo = await streams.todo.get('inbox', todoId)

  if (!existingTodo) {
    logger.warn('Todo not found', { todoId })

    return {
      status: 404,
      body: { error: `Todo with id ${todoId} not found` },
    }
  }

  const updatedTodo = { ...existingTodo }

  if (req.body.checked !== undefined) {
    updatedTodo.completedAt = req.body.checked ? new Date().toISOString() : undefined
  }

  if (req.body.description !== undefined) {
    updatedTodo.description = req.body.description
  }

  if (req.body.dueDate !== undefined) {
    updatedTodo.dueDate = req.body.dueDate
  }

  const result = await streams.todo.set('inbox', todoId, updatedTodo)

  logger.info('Todo updated successfully', { todoId })

  return { status: 200, body: result }
}
