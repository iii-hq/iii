import { useApi, useFunctionsAvailable } from './hooks'
import { streams, logger } from './streams'

useFunctionsAvailable((functions) => {
  console.log('--------------------------------')
  console.log('Functions available:', functions.length)
  console.log('--------------------------------')
  logger.info('App started', { functions_count: functions.length })
})

const LISTS = {
  INBOX: 'inbox',
  TODAY: 'today',
  COMPLETED: 'completed',
}

console.log('🚀 iii Todo App - With Logging')
console.log('📡 APIs: CRUD operations')
console.log('🌊 Streams: Redis-backed state')
console.log('--------------------------------')

useApi(
  { api_path: 'todos', http_method: 'GET', description: 'List all todos', metadata: { tags: ['todo', 'list'] } },
  async (req) => {
    const list = (req.query_params?.list as string) || LISTS.INBOX
    logger.info('Fetching todos', { list })
    const todos = await streams.getGroup('todo', list)
    return { status_code: 200, body: { todos, count: todos.length }, headers: { 'Content-Type': 'application/json' } }
  },
)

useApi(
  { api_path: 'todo/:id', http_method: 'GET', description: 'Get a single todo by ID', metadata: { tags: ['todo', 'read'] } },
  async (req) => {
    const todoId = req.path_params?.id
    if (!todoId) {
      logger.warn('Get todo failed - missing ID')
      return { status_code: 400, body: { error: 'Todo ID is required' } }
    }
    logger.info('Getting todo', { todoId })
    for (const list of Object.values(LISTS)) {
      const todo = await streams.get('todo', list, todoId)
      if (todo) {
        return { status_code: 200, body: todo, headers: { 'Content-Type': 'application/json' } }
      }
    }
    logger.warn('Todo not found', { todoId })
    return { status_code: 404, body: { error: 'Todo not found' } }
  },
)

useApi(
  { api_path: 'todo', http_method: 'POST', description: 'Create a new todo', metadata: { tags: ['todo', 'create'] } },
  async (req) => {
    const { title, description, priority } = req.body
    const todoId = `todo-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`
    if (!title) {
      logger.warn('Create todo failed - missing title')
      return { status_code: 400, body: { error: 'Title is required' } }
    }
    const newTodo = {
      id: todoId,
      title,
      description: description || '',
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      priority: priority || 'medium',
      status: 'pending',
      list: LISTS.INBOX,
    }
    await streams.set('todo', LISTS.INBOX, todoId, newTodo)
    logger.info('Todo created', { todoId, title, priority: newTodo.priority })
    return { status_code: 201, body: newTodo, headers: { 'Content-Type': 'application/json' } }
  },
)

useApi(
  { api_path: 'todo/:id', http_method: 'PUT', description: 'Update a todo', metadata: { tags: ['todo', 'update'] } },
  async (req) => {
    const todoId = req.path_params?.id
    const updates = req.body
    if (!todoId) {
      logger.warn('Update todo failed - missing ID')
      return { status_code: 400, body: { error: 'Todo ID is required' } }
    }
    let existingTodo: any = null
    let currentList: string = LISTS.INBOX
    for (const list of Object.values(LISTS)) {
      const todo = await streams.get('todo', list, todoId)
      if (todo) {
        existingTodo = todo
        currentList = list
        break
      }
    }
    if (!existingTodo) {
      logger.warn('Update todo failed - not found', { todoId })
      return { status_code: 404, body: { error: 'Todo not found' } }
    }
    const updatedTodo = {
      ...existingTodo,
      ...updates,
      id: todoId,
      updatedAt: new Date().toISOString(),
    }
    await streams.set('todo', currentList, todoId, updatedTodo)
    logger.info('Todo updated', { todoId, updates: Object.keys(updates) })
    return { status_code: 200, body: updatedTodo, headers: { 'Content-Type': 'application/json' } }
  },
)

useApi(
  { api_path: 'todo/:id', http_method: 'DELETE', description: 'Delete a todo', metadata: { tags: ['todo', 'delete'] } },
  async (req) => {
    const todoId = req.path_params?.id
    if (!todoId) {
      logger.warn('Delete todo failed - missing ID')
      return { status_code: 400, body: { error: 'Todo ID is required' } }
    }
    for (const list of Object.values(LISTS)) {
      const todo = await streams.get('todo', list, todoId)
      if (todo) {
        await streams.delete('todo', list, todoId)
        logger.info('Todo deleted', { todoId, list })
        return { status_code: 200, body: { success: true, deleted: todoId } }
      }
    }
    logger.warn('Delete todo failed - not found', { todoId })
    return { status_code: 404, body: { error: 'Todo not found' } }
  },
)

useApi(
  { api_path: 'todo/:id/complete', http_method: 'POST', description: 'Mark todo as complete', metadata: { tags: ['todo', 'complete'] } },
  async (req) => {
    const todoId = req.path_params?.id
    if (!todoId) {
      logger.warn('Complete todo failed - missing ID')
      return { status_code: 400, body: { error: 'Todo ID is required' } }
    }
    for (const list of Object.values(LISTS)) {
      const todo = await streams.get('todo', list, todoId)
      if (todo) {
        const completedTodo = {
          ...todo,
          status: 'completed',
          completedAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
          list: LISTS.COMPLETED,
        }
        await streams.delete('todo', list, todoId)
        await streams.set('todo', LISTS.COMPLETED, todoId, completedTodo)
        logger.info('Todo completed', { todoId, previousList: list })
        return { status_code: 200, body: completedTodo, headers: { 'Content-Type': 'application/json' } }
      }
    }
    logger.warn('Complete todo failed - not found', { todoId })
    return { status_code: 404, body: { error: 'Todo not found' } }
  },
)

useApi(
  { api_path: 'stats', http_method: 'GET', description: 'Get todo statistics', metadata: { tags: ['stats'] } },
  async () => {
    const stats: Record<string, number> = {}
    let total = 0
    for (const list of Object.values(LISTS)) {
      const todos = await streams.getGroup('todo', list)
      stats[list] = todos.length
      total += todos.length
    }
    logger.info('Stats retrieved', { total, ...stats })
    return { status_code: 200, body: { stats, total }, headers: { 'Content-Type': 'application/json' } }
  },
)

useApi(
  { api_path: 'health', http_method: 'GET', description: 'Health check', metadata: { tags: ['system'] } },
  async () => {
    return { status_code: 200, body: { status: 'healthy', timestamp: new Date().toISOString() } }
  },
)
