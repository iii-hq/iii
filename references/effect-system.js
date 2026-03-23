/**
 * Pattern: Effect Systems & Typed Functional Infrastructure
 * Comparable to: Effect-TS
 *
 * Demonstrates composable, pipeable function chains where each step
 * is a pure(ish) function registered in iii. Steps are composed
 * by calling one function from another, building a pipeline that
 * is traceable, retryable, and observable end-to-end.
 *
 * How-to references:
 *   - Functions & Triggers: https://iii.dev/docs/how-to/use-functions-and-triggers
 *   - HTTP endpoints:       https://iii.dev/docs/how-to/expose-http-endpoint
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'effect-system',
})

// ---------------------------------------------------------------------------
// Primitive effects — small, composable functions (like Effect-TS layers)
// ---------------------------------------------------------------------------

// Effect: validate and parse input
iii.registerFunction({ id: 'fx::parse-user-input' }, async (data) => {
  if (!data.email || !data.email.includes('@')) {
    throw new Error('ValidationError: invalid email')
  }
  if (!data.name || data.name.trim().length < 2) {
    throw new Error('ValidationError: name too short')
  }

  return {
    email: data.email.toLowerCase().trim(),
    name: data.name.trim(),
    source: data.source || 'unknown',
  }
})

// Effect: check for duplicates
iii.registerFunction({ id: 'fx::check-duplicate' }, async (data) => {
  const existing = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'users', key: data.email },
  })

  if (existing) {
    throw new Error(`DuplicateError: ${data.email} already registered`)
  }

  return data // pass through
})

// Effect: enrich with defaults
iii.registerFunction({ id: 'fx::enrich-user' }, async (data) => {
  return {
    ...data,
    id: `usr-${Date.now()}`,
    role: 'member',
    created_at: new Date().toISOString(),
    preferences: { theme: 'light', notifications: true },
  }
})

// Effect: persist to state
iii.registerFunction({ id: 'fx::persist-user' }, async (data) => {
  await iii.trigger({
    function_id: 'state::set',
    payload: { scope: 'users', key: data.email, value: { _key: data.email, ...data } },
  })
  return data
})

// Effect: send welcome notification (fire-and-forget side effect)
iii.registerFunction({ id: 'fx::send-welcome' }, async (data) => {
  const logger = new Logger()
  logger.info('Sending welcome email', { to: data.email })

  iii.trigger({
    function_id: 'publish',
    payload: {
      topic: 'notifications.send',
      data: {
        type: 'email',
        to: data.email,
        template: 'welcome',
        vars: { name: data.name },
      },
    },
    action: TriggerAction.Void(),
  })

  return data
})

// ---------------------------------------------------------------------------
// Pipeline — compose effects into a single workflow (like Effect.pipe)
//
// Each step calls the next via iii.trigger, which gives us:
//   - Full distributed tracing across all steps
//   - Each step is independently testable and retryable
//   - Errors propagate cleanly (thrown errors bubble up)
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'fx::register-user-pipeline' }, async (rawInput) => {
  const logger = new Logger()
  logger.info('Starting registration pipeline')

  // pipe: parse → check duplicate → enrich → persist → welcome
  const parsed   = await iii.trigger({ function_id: 'fx::parse-user-input', payload: rawInput })
  const checked  = await iii.trigger({ function_id: 'fx::check-duplicate', payload: parsed })
  const enriched = await iii.trigger({ function_id: 'fx::enrich-user', payload: checked })
  const saved    = await iii.trigger({ function_id: 'fx::persist-user', payload: enriched })
  await iii.trigger({ function_id: 'fx::send-welcome', payload: saved })

  logger.info('Pipeline complete', { userId: saved.id })
  return { id: saved.id, email: saved.email, status: 'registered' }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'fx::register-user-pipeline',
  config: { api_path: '/users/register', http_method: 'POST' },
})

// ---------------------------------------------------------------------------
// Composition — reuse the same primitives in a different pipeline
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'fx::import-users-batch' }, async (data) => {
  const logger = new Logger()
  const results = { succeeded: 0, failed: 0, errors: [] }

  for (const user of data.users) {
    try {
      const parsed   = await iii.trigger({ function_id: 'fx::parse-user-input', payload: user })
      const checked  = await iii.trigger({ function_id: 'fx::check-duplicate', payload: parsed })
      const enriched = await iii.trigger({ function_id: 'fx::enrich-user', payload: checked })
      await iii.trigger({ function_id: 'fx::persist-user', payload: enriched })
      results.succeeded++
    } catch (err) {
      results.failed++
      results.errors.push({ email: user.email, error: err.message })
    }
  }

  logger.info('Batch import complete', results)
  return results
})

iii.registerTrigger({
  type: 'http',
  function_id: 'fx::import-users-batch',
  config: { api_path: '/users/import', http_method: 'POST' },
})
