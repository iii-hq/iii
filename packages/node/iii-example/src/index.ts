import { bridge } from './bridge'
import { logger } from './logger'
import { ApiResponse, ApiRequest } from 'iii'


bridge.registerFunction(
  {
    functionPath: 'register.user',
    description: 'simple user registration function',

  },
  async (request: ApiRequest<{ email: string; username: string, data: any }>): Promise<ApiResponse> => {
    const { body: payload, pathParams, queryParams, headers, method } = request;
    console.log({ payload, pathParams, queryParams, headers, method });
    // In a real implementation, this would emit to the bridge/event system
    logger.info(`User registered with username: ${payload.username} and email: ${payload.email}`);
    const body = {
      userId: 'user_' + Math.random().toString(36).substr(2, 9),
      username: payload.username,
      email: payload.email,
    };
    return {
      status_code: 201,
      headers: { 'Content-Type': 'application/json' },
      body: body,
    };
  },
)

bridge.registerTrigger({
  triggerType: 'api',
  functionPath: 'register.user',
  config: {
    apiPath: 'register/user',
    httpMethod: 'POST',
  },
})
bridge.registerFunction({ functionPath: 'service.echo' }, async (payload) => {
  console.log('Echoing message', payload)
  return { ...payload, from: 'service.echo' }
})

bridge.registerFunction({ functionPath: 'test' }, async (payload) => {
  const response = await bridge.invokeFunction('service.echo', payload)
  console.log('Response from service.echo', response)
  return response
})

// const handlers: Record<string, () => Promise<void>> = {}
bridge.registerTrigger({
  triggerType: 'api',
  functionPath: 'engine.echo',
  config: {
    apiPath: 'echo',
    httpMethod: 'POST',
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

process.stdin.on('data', async (data) => {
  const topic = data.toString().trim()
  console.log('Emitting event', topic)
  await bridge.invokeFunction('emit', { topic, data: { text: topic } })
  // bridge.invokeFunctionAsync('engine.echo', { text: data.toString().trim() })
  console.log('Event emitted')
})

// bridge.registerTrigger({
//   triggerType: 'every-10-seconds',
//   functionPath: 'engine.echo',
//   config: { name: 'EngineEcho', args: { text: 'Hello, Engine!', from: 'typescript-example' } },
// })
