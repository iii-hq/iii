import { type Handlers, logger, type StepConfig, type UpdateOp } from 'motia'
import { z } from 'zod'
import { todoSchema } from './create-todo.step'
import { todoStream } from './todo.stream'

export const config = {
  name: 'UpdateTodo',
  description: 'Update an existing todo item',
  flows: ['todo-app'],
  triggers: [
    {
      type: 'http',
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
    },
  ],
  enqueues: [],
  virtualSubscribes: ['todo-created'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  const { todoId } = request.pathParams || {}
  const body = request.body || {}

  logger.info('Updating todo', { todoId, body })

  const updateOps: UpdateOp[] = []

  if (body.checked !== undefined) {
    updateOps.push({ type: 'set', path: 'completedAt', value: body.checked ? new Date().toISOString() : undefined })
  }

  if (body.description !== undefined) {
    updateOps.push({ type: 'set', path: 'description', value: body.description })
  }

  if (body.dueDate !== undefined) {
    updateOps.push({ type: 'set', path: 'dueDate', value: body.dueDate })
  }

  if (updateOps.length === 0) {
    return { status: 400, body: { error: 'No fields to update' } }
  }

  const result = await todoStream.update('inbox', todoId, updateOps)

  if (!result.old_value) {
    return { status: 404, body: { error: `Todo with id ${todoId} not found` } }
  }

  logger.info('Todo updated successfully', { todoId })

  return { status: 200, body: result.new_value }
}
