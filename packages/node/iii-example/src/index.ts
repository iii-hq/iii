import { bridge } from './bridge'
import { type ApiResponse, type ApiRequest, getContext } from 'iii'

bridge.registerFunction(
  {
    functionPath: 'register.user',
    description: 'simple user registration function',
  },
  async (request: ApiRequest<{ email: string; username: string; data: any }>): Promise<ApiResponse> => {
    const context = getContext()
    const { body, pathParams, queryParams, headers, method } = request
    const userId = 'user_' + Math.random().toString(36).substring(2, 9)
    const responseBody = { userId, username: body.username, email: body.email }

    context.logger.info('Request', { body, pathParams, queryParams, headers, method })
    context.logger.info('User registered', { username: body.username, email: body.email })

    return {
      status_code: 201,
      headers: { 'Content-Type': 'application/json' },
      body: responseBody,
    }
  },
)

bridge.registerTrigger({
  triggerType: 'api',
  functionPath: 'register.user',
  config: {
    api_path: 'register/user',
    http_method: 'POST',
  },
})

bridge.registerFunction({ functionPath: 'service.echo' }, async (payload) => {
  const context = getContext()
  context.logger.info('Echoing message', payload)
  return { ...payload, from: 'service.echo' }
})

bridge.registerFunction({ functionPath: 'test' }, async (payload) => {
  const response = await bridge.invokeFunction('service.echo', payload)
  const context = getContext()

  context.logger.info('Response from service.echo', { response: { response } })
  return response
})

// const handlers: Record<string, () => Promise<void>> = {}
bridge.registerTrigger({
  triggerType: 'api',
  functionPath: 'engine.echo',
  config: {
    api_path: 'echo',
    http_method: 'POST',
  },
})

// setInterval(async () => {
//   const handlersSize = Object.keys(handlers).length
//   await Promise.allSettled(Object.values(handlers).map((handler) => handler()))
//   console.log('Handlers executed (', handlersSize, ' handlers)')
// }, 10000)

// bridge.registerTriggerType<{ name: string; args: any }>(
//   { id: 'every-10-seconds', description: 'Every 10 seconds trigger' },
//   {
//     registerTrigger: async ({ id, functionPath, config }) => {
//       handlers[id] = async () => {
//         await bridge.invokeFunction(functionPath, config.args ?? {})
//         console.log(`Result of ${config.name} function in ${functionPath}`)
//       }
//     },
//     unregisterTrigger: async ({ id }) => {
//       delete handlers[id]
//     },
//   },
// )

bridge.registerTrigger({
  triggerType: 'event',
  functionPath: 'test',
  config: { topic: 'echo' },
})

bridge.registerTrigger({
  triggerType: 'event',
  functionPath: 'test',
  config: { topic: 'test' },
})

// Background cleanup function triggered by cron
bridge.registerFunction({ functionPath: 'background.cleanup' }, async (payload) => {
  const context = getContext()
  const timestamp = new Date().toISOString()

  context.logger.info('Running background cleanup', {
    timestamp,
    trigger: payload.trigger,
    job_id: payload.job_id,
    scheduled_time: payload.scheduled_time,
  })

  // Simulate cleanup work
  const cleanedItems = Math.floor(Math.random() * 100)
  context.logger.info('Cleanup completed', { cleanedItems, timestamp })

  return { success: true, cleanedItems, timestamp }
})

// Register cron trigger to run cleanup every 10 seconds
bridge.registerTrigger({
  triggerType: 'cron',
  functionPath: 'background.cleanup',
  config: {
    expression: '*/10 * * * * *', // Every 10 seconds
  },
})

process.stdin.on('data', async (data) => {
  const topic = data.toString().trim()
  const context = getContext()

  context.logger.info('Emitting event', topic)
  await bridge.invokeFunction('emit', { topic, data: { text: topic } })
  // bridge.invokeFunctionAsync('engine.echo', { text: data.toString().trim() })
  context.logger.info('Event emitted')
})

// bridge.registerTrigger({
//   triggerType: 'every-10-seconds',
//   functionPath: 'engine.echo',
//   config: { name: 'EngineEcho', args: { text: 'Hello, Engine!', from: 'typescript-example' } },
// })
