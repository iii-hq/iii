import { ApiRouteConfig, Handlers } from '@iii-dev/motia'
import { z } from 'zod'
import { Todo } from './todo.stream'

export const todoSchema = z.object({
  id: z.string(),
  description: z.string(),
  createdAt: z.string(),
  dueDate: z.string().optional(),
  completedAt: z.string().optional(),
})

export const config: ApiRouteConfig = {
  type: 'api',
  name: 'CreateTodo',
  description: 'Create a new todo item',
  flows: ['todo-app'],

  method: 'POST',
  path: '/todo',
  bodySchema: z.object({
    description: z.string(),
    dueDate: z.string().optional(),
  }),
  responseSchema: {
    200: todoSchema,
    400: z.object({ error: z.string() }),
  },
  emits: [],
  virtualEmits: ['todo-created'],
}

export const handler: Handlers['CreateTodo'] = async (req, { logger, streams }) => {
  logger.info('Creating new todo', { body: req.body })

  const { description, dueDate } = req.body
  const todoId = `todo-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`

  if (!description) {
    return { status: 400, body: { error: 'Description is required' } }
  }

  const newTodo: Todo = {
    id: todoId,
    description,
    createdAt: new Date().toISOString(),
    dueDate: dueDate,
    completedAt: undefined,
  }

  const todo = await streams.todo.set('inbox', todoId, newTodo)

  logger.info('Todo created successfully', { todoId })

  return { status: 200, body: todo }
}
