import { useApi, useFunctionsAvailable } from './hooks'
import { streams } from './streams'

useFunctionsAvailable((functions) => {
  console.log('--------------------------------')
  console.log('Functions available:', functions)
  console.log('--------------------------------')
})

useApi(
  { api_path: 'todo', http_method: 'POST', description: 'Create a new todo', metadata: { tags: ['todo'] } },
  async (req, { logger }) => {
    logger.info('Creating new todo', { body: req.body })

    const { description, dueDate } = req.body
    const todoId = `todo-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`

    if (!description) {
      return { status_code: 400, body: { error: 'Description is required' } }
    }

    const newTodo = {
      id: todoId,
      description,
      createdAt: new Date().toISOString(),
      dueDate: dueDate,
      completedAt: undefined,
    }
    const todo = await streams.set('todo', 'inbox', todoId, newTodo)

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
  async (req, { logger }) => {
    const { todoId } = req.body

    logger.info('Deleting todo', { body: req.body })

    if (!todoId) {
      logger.error('todoId is required')
      return { status_code: 400, body: { error: 'todoId is required' } }
    }

    await streams.delete('todo', 'inbox', todoId)

    logger.info('Todo deleted successfully', { todoId })

    return { status_code: 200, body: { success: true }, headers: { 'Content-Type': 'application/json' } }
  },
)

useApi(
  {
    api_path: 'todo',
    http_method: 'PUT',
    description: 'Update a todo',
    metadata: { tags: ['todo2'] },
  },
  async (req, { logger }) => {
    const { todoId } = req.body
    const existingTodo = todoId ? await streams.get('todo', 'inbox', todoId) : undefined

    logger.info('Updating todo', { body: req.body })

    if (!existingTodo) {
      logger.error('Todo not found')
      return { status_code: 404, body: { error: 'Todo not found' } }
    }

    const todo = await streams.set('todo', 'inbox', todoId, { ...existingTodo, ...req.body })
    logger.info('Todo updated successfully', { todoId })

    return { status_code: 200, body: todo, headers: { 'Content-Type': 'application/json' } }
  },
)
