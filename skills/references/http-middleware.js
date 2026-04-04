import { registerWorker, Logger } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134', { workerName: 'middleware-worker' })
const logger = new Logger(undefined, 'middleware')

iii.registerFunction('middleware::auth', async (req) => {
  const authHeader = req.request?.headers?.authorization
  if (!authHeader || !authHeader.startsWith('Bearer ')) {
    logger.warn('Auth rejected: missing or invalid token')
    return {
      action: 'respond',
      response: {
        status_code: 401,
        body: { error: 'Missing or invalid authorization header' },
      },
    }
  }
  logger.info('Auth passed')
  return { action: 'continue' }
})

iii.registerFunction('middleware::request-logger', async (req) => {
  logger.info('Incoming request', {
    method: req.request?.method,
    path: req.request?.path_params,
    query: req.request?.query_params,
  })
  return { action: 'continue' }
})

iii.registerFunction('api::health', async () => ({
  status_code: 200,
  body: { status: 'ok', timestamp: new Date().toISOString() },
}))

iii.registerTrigger({
  type: 'http',
  function_id: 'api::health',
  config: { api_path: '/health', http_method: 'GET' },
})

iii.registerFunction('api::users-list', async () => ({
  status_code: 200,
  body: { users: [{ id: '1', name: 'Alice' }, { id: '2', name: 'Bob' }] },
}))

iii.registerTrigger({
  type: 'http',
  function_id: 'api::users-list',
  config: {
    api_path: '/users',
    http_method: 'GET',
    middleware_function_ids: ['middleware::request-logger', 'middleware::auth'],
  },
})

iii.registerFunction('api::users-create', async (req) => {
  const { name, email } = req.body ?? {}
  if (!name || !email) {
    return { status_code: 400, body: { error: 'name and email are required' } }
  }
  return { status_code: 201, body: { id: crypto.randomUUID(), name, email } }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'api::users-create',
  config: {
    api_path: '/users',
    http_method: 'POST',
    middleware_function_ids: ['middleware::auth'],
  },
})
