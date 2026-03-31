/**
 * Pattern: Reactive Backend
 * Comparable to: Convex, Firebase, Supabase, Appwrite
 *
 * Demonstrates a real-time todo app backend where state changes
 * automatically trigger side effects (notifications, metrics) and
 * clients receive live updates via streams.
 *
 * How-to references:
 *   - State management: https://iii.dev/docs/how-to/manage-state
 *   - State reactions:  https://iii.dev/docs/how-to/react-to-state-changes
 *   - Streams:          https://iii.dev/docs/how-to/stream-realtime-data
 *   - HTTP endpoints:   https://iii.dev/docs/how-to/expose-http-endpoint
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'reactive-backend',
})

// ---------------------------------------------------------------------------
// CRUD — HTTP endpoints that write to state (the "database")
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'todos::create' }, async (data) => {
  const id = `todo-${Date.now()}`
  const todo = {
    _key: id,
    id,
    title: data.title,
    completed: false,
    created_at: new Date().toISOString(),
  }

  await iii.trigger({
    function_id: 'state::set',
    payload: { scope: 'todos', key: id, value: todo },
  })

  return todo
})

iii.registerFunction({ id: 'todos::toggle' }, async (data) => {
  const todo = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'todos', key: data.id },
  })

  if (!todo) throw new Error(`Todo ${data.id} not found`)

  await iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'todos',
      key: data.id,
      ops: [
        { type: 'set', path: 'completed', value: !todo.completed },
        { type: 'set', path: 'updated_at', value: new Date().toISOString() },
      ],
    },
  })

  return { id: data.id, completed: !todo.completed }
})

iii.registerFunction({ id: 'todos::list' }, async () => {
  return await iii.trigger({
    function_id: 'state::list',
    payload: { scope: 'todos' },
  })
})

iii.registerFunction({ id: 'todos::delete' }, async (data) => {
  await iii.trigger({
    function_id: 'state::delete',
    payload: { scope: 'todos', key: data.id },
  })
  return { deleted: data.id }
})

// HTTP triggers
iii.registerTrigger({ type: 'http', function_id: 'todos::create', config: { api_path: '/todos', http_method: 'POST' } })
iii.registerTrigger({ type: 'http', function_id: 'todos::list', config: { api_path: '/todos', http_method: 'GET' } })
iii.registerTrigger({ type: 'http', function_id: 'todos::toggle', config: { api_path: '/todos/toggle', http_method: 'POST' } })
iii.registerTrigger({ type: 'http', function_id: 'todos::delete', config: { api_path: '/todos/delete', http_method: 'POST' } })

// ---------------------------------------------------------------------------
// Reactive side effect — push changes to connected clients via stream
// Fires automatically whenever ANY todo in the 'todos' scope changes.
// Clients connect via: ws://localhost:3112/stream/todos-live/all
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'todos::on-change' }, async (event) => {
  const { new_value, old_value, key } = event
  const logger = new Logger()

  const action = !old_value ? 'created' : !new_value ? 'deleted' : 'updated'
  logger.info('Todo changed', { key, action })

  // Push the change to all connected clients
  iii.trigger({
    function_id: 'stream::send',
    payload: {
      stream_name: 'todos-live',
      group_id: 'all',
      id: `change-${Date.now()}`,
      event_type: 'todo_changed',
      data: { action, key, todo: new_value },
    },
    action: TriggerAction.Void(),
  })

  return { action, key }
})

iii.registerTrigger({
  type: 'state',
  function_id: 'todos::on-change',
  config: { scope: 'todos' },
})

// ---------------------------------------------------------------------------
// Reactive side effect — update aggregate metrics on any change
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'todos::update-metrics' }, async (event) => {
  const { new_value, old_value } = event

  const ops = []

  // New todo created
  if (new_value && !old_value) {
    ops.push({ type: 'increment', path: 'total', by: 1 })
  }

  // Todo deleted
  if (!new_value && old_value) {
    ops.push({ type: 'increment', path: 'total', by: -1 })
    if (old_value.completed) {
      ops.push({ type: 'increment', path: 'completed', by: -1 })
    }
  }

  // Todo toggled
  if (new_value && old_value && new_value.completed !== old_value.completed) {
    ops.push({
      type: 'increment',
      path: 'completed',
      by: new_value.completed ? 1 : -1,
    })
  }

  if (ops.length > 0) {
    await iii.trigger({
      function_id: 'state::update',
      payload: { scope: 'todo-metrics', key: 'global', ops },
    })
  }
})

iii.registerTrigger({
  type: 'state',
  function_id: 'todos::update-metrics',
  config: { scope: 'todos' },
})

// Expose metrics via HTTP
iii.registerFunction({ id: 'todos::get-metrics' }, async () => {
  return await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'todo-metrics', key: 'global' },
  })
})

iii.registerTrigger({
  type: 'http',
  function_id: 'todos::get-metrics',
  config: { api_path: '/todos/metrics', http_method: 'GET' },
})
