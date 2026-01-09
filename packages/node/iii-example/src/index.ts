import { useApi, useFunctionsAvailable, useCron, useEvent, useOnJoin, useOnLeave } from './hooks'
import { streams, logger, emit } from './streams'

useFunctionsAvailable((functions) => {
  console.log('--------------------------------')
  console.log('Functions available:', functions.length)
  console.log('--------------------------------')
  logger.info('App started', { functions_count: functions.length })
  logger.debug('Debug: Function registration complete', { functions: functions.map(f => f.path) })
})

const LISTS = {
  INBOX: 'inbox',
  TODAY: 'today',
  COMPLETED: 'completed',
}

// Event Topics
const EVENTS = {
  TODO_CREATED: 'todo.created',
  TODO_COMPLETED: 'todo.completed',
  TODO_DELETED: 'todo.deleted',
  TODO_UPDATED: 'todo.updated',
  DAILY_SUMMARY: 'todo.daily_summary',
  HIGH_PRIORITY_ALERT: 'todo.high_priority',
}

console.log('🚀 iii Todo App - Full Demo')
console.log('📡 APIs: CRUD operations')
console.log('🌊 Streams: Redis-backed state')
console.log('⏰ Cron: Scheduled jobs')
console.log('📨 Events: Pub/sub messaging')
console.log('🔗 Workflows: Event-driven chains')
console.log('--------------------------------')

// Cron job: Archive completed todos older than 5 minutes (runs every 15 seconds)
useCron(
  { expression: '*/15 * * * * *', description: 'Archive old completed todos' },
  async (event, context) => {
    logger.debug('Cron: Archive job triggered', { job_id: event.job_id })
    logger.info('Running archive cron job', { job_id: event.job_id })
    
    try {
      const completed = await streams.getGroup('todo', LISTS.COMPLETED)
      logger.debug('Cron: Found completed todos', { count: completed.length })
      
      const fiveMinutesAgo = Date.now() - (5 * 60 * 1000)
      let archivedCount = 0
      
      for (const todo of completed) {
        const completedAt = new Date((todo as any).completedAt).getTime()
        if (completedAt < fiveMinutesAgo) {
          await streams.delete('todo', LISTS.COMPLETED, (todo as any).id)
          archivedCount++
        }
      }
      
      if (archivedCount > 0) {
        logger.info('Archived old todos', { count: archivedCount })
      } else {
        logger.info('No old todos to archive')
      }
    } catch (err) {
      logger.error('Archive cron job failed', { error: String(err), job_id: event.job_id })
    }
  }
)

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
      logger.error('Get todo failed - missing ID in request', { path: req.path })
      return { status_code: 400, body: { error: 'Todo ID is required' } }
    }
    logger.debug('Looking up todo', { todoId, searching_lists: Object.values(LISTS) })
    for (const list of Object.values(LISTS)) {
      const todo = await streams.get('todo', list, todoId)
      if (todo) {
        logger.debug('Todo found', { todoId, list })
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
    logger.debug('Create todo request received', { body: req.body })
    const { title, description, priority } = req.body
    const todoId = `todo-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`
    if (!title) {
      logger.error('Create todo failed - title is required', { body: req.body })
      return { status_code: 400, body: { error: 'Title is required' } }
    }
    if (priority && !['low', 'medium', 'high'].includes(priority)) {
      logger.warn('Invalid priority value, defaulting to medium', { provided: priority })
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
    
    // Emit event for workflow triggers
    await emit(EVENTS.TODO_CREATED, { todo: newTodo, timestamp: new Date().toISOString() })
    
    // If high priority, emit special alert
    if (priority === 'high') {
      await emit(EVENTS.HIGH_PRIORITY_ALERT, { todo: newTodo, message: 'High priority todo created!' })
    }
    
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
        
        // Emit delete event
        await emit(EVENTS.TODO_DELETED, { todoId, todo, list, timestamp: new Date().toISOString() })
        
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
        
        // Emit completion event for workflow triggers
        await emit(EVENTS.TODO_COMPLETED, { 
          todo: completedTodo, 
          previousList: list,
          completionTime: new Date().toISOString() 
        })
        
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
    logger.debug('Health check requested')
    return { status_code: 200, body: { status: 'healthy', timestamp: new Date().toISOString() } }
  },
)

// Demo endpoint to show all log levels
useApi(
  { api_path: 'demo/logs', http_method: 'POST', description: 'Demo all log levels', metadata: { tags: ['demo'] } },
  async (req) => {
    const { message = 'Test message', simulateError = false } = req.body || {}
    
    logger.debug('Debug: Starting log demo', { message, timestamp: Date.now() })
    logger.info('Info: Processing request', { message, source: 'demo' })
    logger.warn('Warning: This is a demo warning', { message, level: 'warn' })
    
    if (simulateError) {
      logger.error('Error: Simulated error occurred!', { 
        message, 
        error_code: 'DEMO_ERROR',
        stack: 'at demo/logs handler'
      })
      return { status_code: 500, body: { error: 'Simulated error', message } }
    }
    
    logger.info('Info: Log demo completed successfully', { levels_shown: ['debug', 'info', 'warn'] })
    return { 
      status_code: 200, 
      body: { 
        success: true, 
        message: 'Check logs page for all log levels!',
        levels: ['debug (grey)', 'info (cyan)', 'warn (yellow)', 'error (red - use simulateError: true)']
      } 
    }
  },
)

// ============================================
// EVENT LISTENERS - Workflow Triggers
// ============================================

// Listen for todo created events - Example workflow: auto-categorize
useEvent(
  { topic: EVENTS.TODO_CREATED, description: 'Process new todos - auto-categorize based on keywords' },
  async (event, context) => {
    const { todo } = event.data
    logger.info('Event received: Todo created', { todoId: todo.id, title: todo.title })
    
    // Example workflow: auto-tag based on keywords
    const urgentKeywords = ['urgent', 'asap', 'immediately', 'critical']
    const isUrgent = urgentKeywords.some(kw => 
      todo.title.toLowerCase().includes(kw) || 
      (todo.description && todo.description.toLowerCase().includes(kw))
    )
    
    if (isUrgent && todo.priority !== 'high') {
      // Auto-escalate priority
      const updated = { ...todo, priority: 'high', autoEscalated: true }
      await streams.set('todo', LISTS.INBOX, todo.id, updated)
      logger.info('Auto-escalated todo priority', { todoId: todo.id, reason: 'urgent keywords detected' })
      await emit(EVENTS.HIGH_PRIORITY_ALERT, { todo: updated, reason: 'auto-escalated' })
    }
  }
)

// Listen for high priority alerts - Example: notification workflow
useEvent(
  { topic: EVENTS.HIGH_PRIORITY_ALERT, description: 'Handle high priority alerts' },
  async (event, context) => {
    const { todo, reason, message } = event.data
    logger.warn('High priority alert!', { 
      todoId: todo.id, 
      title: todo.title, 
      reason: reason || 'manual',
      message 
    })
    
    // Store alert in a separate stream for notifications
    const alertId = `alert-${Date.now()}`
    await streams.set('alerts', 'high_priority', alertId, {
      id: alertId,
      todoId: todo.id,
      todoTitle: todo.title,
      reason: reason || message || 'High priority todo',
      createdAt: new Date().toISOString(),
      acknowledged: false,
    })
    
    logger.info('Alert stored', { alertId, todoId: todo.id })
  }
)

// Listen for todo completed events - Example: gamification/stats workflow
useEvent(
  { topic: EVENTS.TODO_COMPLETED, description: 'Track completed todos for stats' },
  async (event, context) => {
    const { todo, completionTime } = event.data
    logger.info('Event received: Todo completed', { todoId: todo.id })
    
    // Update completion stats
    const today = new Date().toISOString().split('T')[0]
    const statsKey = `stats-${today}`
    const existingStats = await streams.get('analytics', 'daily', statsKey) as any
    
    const stats = existingStats || { 
      id: statsKey,
      date: today, 
      completed: 0, 
      created: 0,
      avgCompletionTimeMs: 0,
    }
    
    // Calculate completion time in ms
    const createdAt = new Date(todo.createdAt).getTime()
    const completedAt = new Date(completionTime).getTime()
    const completionTimeMs = completedAt - createdAt
    
    // Update running average
    stats.completed += 1
    stats.avgCompletionTimeMs = Math.round(
      ((stats.avgCompletionTimeMs * (stats.completed - 1)) + completionTimeMs) / stats.completed
    )
    
    await streams.set('analytics', 'daily', statsKey, stats)
    logger.info('Updated daily stats', { date: today, completed: stats.completed })
  }
)

// ============================================
// STREAM EVENTS - Real-time Connection Handling
// ============================================

// Handle new stream subscribers
useOnJoin(
  { description: 'Handle new stream connections' },
  async (event, context) => {
    logger.info('User joined stream', { 
      stream: event.stream_name, 
      userId: event.user_id,
      timestamp: event.timestamp 
    })
    
    // Track active connections
    const connectionId = `conn-${event.user_id}-${Date.now()}`
    await streams.set('connections', 'active', connectionId, {
      id: connectionId,
      userId: event.user_id,
      stream: event.stream_name,
      joinedAt: event.timestamp,
    })
  }
)

// Handle stream disconnections
useOnLeave(
  { description: 'Handle stream disconnections' },
  async (event, context) => {
    logger.info('User left stream', { 
      stream: event.stream_name, 
      userId: event.user_id 
    })
  }
)

// ============================================
// ADDITIONAL CRON JOBS
// ============================================

// Daily summary cron - runs at midnight
useCron(
  { expression: '0 0 * * *', description: 'Generate daily todo summary' },
  async (event, context) => {
    logger.info('Running daily summary job')
    
    const stats: Record<string, number> = {}
    let total = 0
    for (const list of Object.values(LISTS)) {
      const todos = await streams.getGroup('todo', list)
      stats[list] = todos.length
      total += todos.length
    }
    
    // Emit summary event
    await emit(EVENTS.DAILY_SUMMARY, {
      date: new Date().toISOString().split('T')[0],
      stats,
      total,
      generatedAt: new Date().toISOString(),
    })
    
    logger.info('Daily summary generated', { total, ...stats })
  }
)

// ============================================
// ANALYTICS API
// ============================================

useApi(
  { api_path: 'analytics/daily', http_method: 'GET', description: 'Get daily analytics', metadata: { tags: ['analytics'] } },
  async (req) => {
    const date = (req.query_params?.date as string) || new Date().toISOString().split('T')[0]
    const statsKey = `stats-${date}`
    const stats = await streams.get('analytics', 'daily', statsKey)
    
    if (!stats) {
      return { status_code: 200, body: { date, completed: 0, created: 0, avgCompletionTimeMs: 0 } }
    }
    
    return { status_code: 200, body: stats, headers: { 'Content-Type': 'application/json' } }
  },
)

useApi(
  { api_path: 'alerts', http_method: 'GET', description: 'Get high priority alerts', metadata: { tags: ['alerts'] } },
  async (req) => {
    const alerts = await streams.getGroup('alerts', 'high_priority')
    const unacknowledged = alerts.filter((a: any) => !a.acknowledged)
    return { 
      status_code: 200, 
      body: { alerts: unacknowledged, total: alerts.length, unacknowledged: unacknowledged.length },
      headers: { 'Content-Type': 'application/json' } 
    }
  },
)

useApi(
  { api_path: 'alert/:id/acknowledge', http_method: 'POST', description: 'Acknowledge an alert', metadata: { tags: ['alerts'] } },
  async (req) => {
    const alertId = req.path_params?.id
    if (!alertId) {
      return { status_code: 400, body: { error: 'Alert ID is required' } }
    }
    
    const alert = await streams.get('alerts', 'high_priority', alertId) as any
    if (!alert) {
      return { status_code: 404, body: { error: 'Alert not found' } }
    }
    
    const updated = { ...alert, acknowledged: true, acknowledgedAt: new Date().toISOString() }
    await streams.set('alerts', 'high_priority', alertId, updated)
    logger.info('Alert acknowledged', { alertId })
    
    return { status_code: 200, body: updated, headers: { 'Content-Type': 'application/json' } }
  },
)
