import { useApi } from './hooks'
import { state } from './state'
import { streams } from './stream'
import type { Todo } from './types'
import './http-example'
import './iii-zod-example'
import './queue-basic'
import './queue-ecommerce'
import './queue-bulk-email'
import './queue-financial'
import './queue-dlq'
import './trigger-types'

useApi(
  {
    api_path: 'error-test',
    http_method: 'GET',
    description: 'Throws an error to test OTEL stack traces',
  },
  async () => {
    throw new Error('Intentional error for OTEL stacktrace testing')
  },
)

useApi(
  {
    api_path: '/todo',
    http_method: 'POST',
    description: 'Create a new todo',
    metadata: { tags: ['todo'] },
  },
  async (req, logger) => {
    logger.info('Creating new todo', { body: req.body })

    const { description, dueDate } = req.body
    const todoId = `todo-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`

    if (!description) {
      return { status_code: 400, body: { error: 'Description is required' } }
    }

    const newTodo: Todo = {
      id: todoId,
      description,
      groupId: 'inbox',
      createdAt: new Date().toISOString(),
      dueDate: dueDate,
      completedAt: null,
    }
    const todo = await streams.set<Todo>('todo', 'inbox', todoId, newTodo)

    return { status_code: 201, body: todo, headers: { 'Content-Type': 'application/json' } }
  },
)

useApi(
  {
    api_path: 'todo',
    http_method: 'DELETE',
    description: 'Delete a todo',
    metadata: { tags: ['todo'] },
  },
  async (req, logger) => {
    const { todoId } = req.body

    logger.info('Deleting todo', { body: req.body })

    if (!todoId) {
      logger.error('todoId is required')
      return { status_code: 400, body: { error: 'todoId is required' } }
    }

    await streams.delete('todo', 'inbox', todoId)

    logger.info('Todo deleted successfully', { todoId })

    return {
      status_code: 200,
      body: { success: true },
      headers: { 'Content-Type': 'application/json' },
    }
  },
)

useApi(
  {
    api_path: 'todo/:id',
    http_method: 'PUT',
    description: 'Update a todo',
    metadata: { tags: ['todo2'] },
  },
  async (req, logger) => {
    const todoId = req.path_params.id
    const existingTodo = todoId ? await streams.get<Todo | null>('todo', 'inbox', todoId) : null

    logger.info('Updating todo', { body: req.body, todoId })

    if (!existingTodo) {
      logger.error('Todo not found')
      return { status_code: 404, body: { error: 'Todo not found' } }
    }

    const todo = await streams.set<Todo>('todo', 'inbox', todoId, { ...existingTodo, ...req.body })

    logger.info('Todo updated successfully', { todoId })

    return { status_code: 200, body: todo, headers: { 'Content-Type': 'application/json' } }
  },
)

useApi({ api_path: 'state', http_method: 'POST', description: 'Set application state' }, async (req, logger) => {
  logger.info('Creating new todo', { body: req.body })

  const { description, dueDate } = req.body
  const todoId = `todo-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`

  if (!description) {
    return { status_code: 400, body: { error: 'Description is required' } }
  }

  const newTodo: Todo = {
    id: todoId,
    description,
    groupId: 'inbox',
    createdAt: new Date().toISOString(),
    dueDate: dueDate,
    completedAt: null,
  }
  const todo = await state.set<Todo>({ scope: 'todo', key: todoId, value: newTodo })

  return { status_code: 201, body: todo, headers: { 'Content-Type': 'application/json' } }
})

useApi({ api_path: 'state/:id', http_method: 'GET', description: 'Get state by ID' }, async (req, logger) => {
  logger.info('Getting todo', { ...req.path_params })

  const todoId = req.path_params.id
  const todo = await state.get<Todo | null>({ scope: 'todo', key: todoId })

  return { status_code: 200, body: todo, headers: { 'Content-Type': 'application/json' } }
})
