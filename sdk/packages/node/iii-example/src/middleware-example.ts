import { Logger } from 'iii-sdk'
import { iii } from './iii'

const logger = new Logger(undefined, 'middleware-example')

// ─────────────────────────────────────────────────────────────────────
// Middleware functions
// ─────────────────────────────────────────────────────────────────────

// Auth middleware: checks for a valid API key in the Authorization header.
// Returns 401 if missing or invalid, otherwise continues to the next step.
iii.registerFunction('middleware::auth', async req => {
  const authHeader = req.request?.headers?.authorization
  if (!authHeader || !authHeader.startsWith('Bearer ')) {
    logger.warn('Auth middleware rejected request: missing or invalid token')
    return {
      action: 'respond',
      response: {
        status_code: 401,
        body: { error: 'Missing or invalid authorization header' },
      },
    }
  }
  logger.info('Auth middleware passed')
  return { action: 'continue' }
})

// Request logger middleware: logs method and path, then continues.
iii.registerFunction('middleware::request-logger', async req => {
  logger.info('Incoming request', {
    method: req.request?.method,
    path: Object.keys(req.request?.path_params ?? {}).length > 0 ? req.request.path_params : 'none',
    query: req.request?.query_params,
  })
  return { action: 'continue' }
})

// ─────────────────────────────────────────────────────────────────────
// Handler functions
// ─────────────────────────────────────────────────────────────────────

// Public endpoint: no middleware
iii.registerFunction('api::health', async () => ({
  status_code: 200,
  body: { status: 'ok', timestamp: new Date().toISOString() },
}))

iii.observe(
  { function_id: 'http::on-request', args: { method: 'GET', path: '/health' } },
  'api::health',
)

// Protected endpoint: auth + logger middleware
iii.registerFunction('api::users-list', async () => ({
  status_code: 200,
  body: {
    users: [
      { id: '1', name: 'Alice' },
      { id: '2', name: 'Bob' },
    ],
  },
}))

iii.observe(
  {
    function_id: 'http::on-request',
    args: {
      method: 'GET',
      path: '/users',
      middleware_function_ids: ['middleware::request-logger', 'middleware::auth'],
    },
  },
  'api::users-list',
)

// Protected endpoint: auth only
iii.registerFunction('api::users-create', async req => {
  const { name, email } = req.body ?? {}
  if (!name || !email) {
    return { status_code: 400, body: { error: 'name and email are required' } }
  }
  const user = { id: crypto.randomUUID(), name, email }
  logger.info('User created', { userId: user.id })
  return { status_code: 201, body: user }
})

iii.observe(
  {
    function_id: 'http::on-request',
    args: { method: 'POST', path: '/users', middleware_function_ids: ['middleware::auth'] },
  },
  'api::users-create',
)

logger.info(
  'Middleware example registered: GET /health (public), GET /users (auth+logger), POST /users (auth)',
)
