/**
 * Pattern: HTTP Endpoints
 * Comparable to: Express, Fastify, Flask
 *
 * Exposes RESTful HTTP endpoints backed by iii functions.
 * Each handler receives an ApiRequest object and returns
 * { status_code, body, headers }.
 *
 * How-to references:
 *   - HTTP endpoints: https://iii.dev/docs/how-to/expose-http-endpoint
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'http-endpoints',
})

// ---------------------------------------------------------------------------
// POST /users — Create a new user
// ApiRequest: { body, path_params, headers, method }
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'users::create' }, async (req) => {
  const logger = new Logger()
  const { name, email } = req.body
  const id = `usr-${Date.now()}`

  const user = { id, name, email, created_at: new Date().toISOString() }

  await iii.trigger({
    function_id: 'state::set',
    payload: { scope: 'users', key: id, value: user },
  })

  logger.info('User created', { id, email })

  return { status_code: 201, body: user, headers: { 'Content-Type': 'application/json' } }
})

// ---------------------------------------------------------------------------
// GET /users/:id — Retrieve a user by path parameter
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'users::get-by-id' }, async (req) => {
  const { id } = req.path_params

  const user = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'users', key: id },
  })

  if (!user) {
    return { status_code: 404, body: { error: 'User not found' } }
  }

  return { status_code: 200, body: user }
})

// ---------------------------------------------------------------------------
// GET /users — List all users
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'users::list' }, async () => {
  const users = await iii.trigger({
    function_id: 'state::list',
    payload: { scope: 'users' },
  })

  return { status_code: 200, body: users }
})

// ---------------------------------------------------------------------------
// PUT /users/:id — Update an existing user
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'users::update' }, async (req) => {
  const { id } = req.path_params
  const updates = req.body

  const existing = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'users', key: id },
  })

  if (!existing) {
    return { status_code: 404, body: { error: 'User not found' } }
  }

  const ops = Object.entries(updates).map(([path, value]) => ({
    type: 'set',
    path,
    value,
  }))

  ops.push({ type: 'set', path: 'updated_at', value: new Date().toISOString() })

  await iii.trigger({
    function_id: 'state::update',
    payload: { scope: 'users', key: id, ops },
  })

  return { status_code: 200, body: { id, ...updates } }
})

// ---------------------------------------------------------------------------
// DELETE /users/:id — Remove a user
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'users::delete' }, async (req) => {
  const { id } = req.path_params

  const existing = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'users', key: id },
  })

  if (!existing) {
    return { status_code: 404, body: { error: 'User not found' } }
  }

  await iii.trigger({
    function_id: 'state::delete',
    payload: { scope: 'users', key: id },
  })

  return { status_code: 204, body: null }
})

// ---------------------------------------------------------------------------
// HTTP trigger registrations
// ---------------------------------------------------------------------------
iii.registerTrigger({ type: 'http', function_id: 'users::create', config: { api_path: '/users', http_method: 'POST' } })
iii.registerTrigger({ type: 'http', function_id: 'users::get-by-id', config: { api_path: '/users/:id', http_method: 'GET' } })
iii.registerTrigger({ type: 'http', function_id: 'users::list', config: { api_path: '/users', http_method: 'GET' } })
iii.registerTrigger({ type: 'http', function_id: 'users::update', config: { api_path: '/users/:id', http_method: 'PUT' } })
iii.registerTrigger({ type: 'http', function_id: 'users::delete', config: { api_path: '/users/:id', http_method: 'DELETE' } })
