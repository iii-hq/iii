import { bridge } from './bridge'
import { type ApiResponse, type ApiRequest, getContext, registerStep, type StepConfig } from '@iii-dev/sdk'
import { streams } from './streams'

const createTodoConfig: StepConfig = {
  name: 'create-todo',
  triggers: [
    {
      type: 'api',
      path: '/todo',
      method: 'POST',
    },
  ],
  emits: [],
}

async function createTodoHandler(req: ApiRequest<any>): Promise<ApiResponse> {
  const { logger } = getContext()
  logger.info('Creating new todo', { body: req.body })

  const { description, dueDate } = req.body

  if (!description) {
    return { status_code: 400, body: { error: 'Description is required' } }
  }

  const todoId = `todo-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`
  const newTodo = {
    id: todoId,
    description,
    createdAt: new Date().toISOString(),
    dueDate: dueDate,
    completedAt: undefined,
  }

  const todo = await streams.set('todo', 'inbox', todoId, newTodo)

  return { status_code: 201, body: todo, headers: { 'Content-Type': 'application/json' } }
}

bridge.registerFunction({ function_path: createTodoConfig.name }, createTodoHandler)
registerStep(bridge, createTodoConfig, createTodoConfig.name)

const deleteTodoConfig: StepConfig = {
  name: 'delete-todo',
  triggers: [
    {
      type: 'api',
      path: '/todo',
      method: 'DELETE',
    },
  ],
  emits: [],
}

async function deleteTodoHandler(req: ApiRequest<any>): Promise<ApiResponse> {
  const { logger } = getContext()
  const { todoId } = req.body

  logger.info('Deleting todo', { body: req.body })

  if (!todoId) {
    logger.error('todoId is required')
    return { status_code: 400, body: { error: 'todoId is required' } }
  }

  await streams.delete('todo', 'inbox', todoId)
  logger.info('Todo deleted successfully', { todoId })

  return { status_code: 200, body: { success: true }, headers: { 'Content-Type': 'application/json' } }
}

bridge.registerFunction({ function_path: deleteTodoConfig.name }, deleteTodoHandler)
registerStep(bridge, deleteTodoConfig, deleteTodoConfig.name)

const multiTriggerConfig: StepConfig = {
  name: 'process-todos',
  triggers: [
    {
      type: 'event',
      subscribes: ['todo.created'],
    },
    {
      type: 'cron',
      expression: '0 * * * *',
    },
    {
      type: 'api',
      path: '/todos/process',
      method: 'POST',
    },
  ],
  emits: ['todo.processed'],
  description: 'Process todos triggered by events, cron, or API',
}

async function processTodosHandler(data: any): Promise<any> {
  const { logger } = getContext()
  logger.info('Processing todos', { data })
  return { success: true }
}

bridge.registerFunction({ function_path: multiTriggerConfig.name }, processTodosHandler)
registerStep(bridge, multiTriggerConfig, multiTriggerConfig.name)
