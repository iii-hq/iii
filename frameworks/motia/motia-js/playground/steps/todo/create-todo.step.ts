import { type Handlers, http, logger, type StepConfig, stateManager } from 'motia'
import { z } from 'zod'
import { type Todo, todoStream } from './todo.stream'

export const todoSchema = z.object({
  id: z.string(),
  description: z.string(),
  createdAt: z.string(),
  dueDate: z.string().optional(),
  completedAt: z.string().optional(),
})

export const config = {
  name: 'CreateTodo',
  description: 'Create a new todo item',
  flows: ['todo-app'],
  triggers: [
    http('POST', '/todo', {
      bodySchema: z.object({ description: z.string(), dueDate: z.string().optional() }),
      responseSchema: {
        200: todoSchema,
        400: z.object({ error: z.string() }),
      },
    }),
  ],
  enqueues: [],
  virtualEnqueues: ['todo-created'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  logger.info('Creating new todo', { body: request.body })

  const { description, dueDate } = request.body || {}
  const todoId = `todo-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`

  if (!description) {
    return { status: 400, body: { error: 'Description is required' } }
  }

  const newTodo: Todo = {
    id: todoId,
    description,
    createdAt: new Date().toISOString(),
    dueDate,
  }

  const todo = await todoStream.set('inbox', todoId, newTodo)

  await stateManager.set('todos', todoId, newTodo)

  logger.info('Todo created successfully', { todoId })

  return { status: 200, body: todo.new_value }
}
