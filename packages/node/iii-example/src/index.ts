import { bridge } from './bridge'
import { logger } from './logger'

bridge.registerFunction({ functionPath: 'service.echo' }, async (payload) => {
  console.log('Echoing message', payload)
  return payload
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

process.stdin.on('data', async (data) => {
  console.log('Emitting event', data.toString().trim())
  await bridge.invokeFunction('emit', { topic: 'echo', data: { text: data.toString().trim() } })
  // bridge.invokeFunctionAsync('engine.echo', { text: data.toString().trim() })
  console.log('Event emitted')
})

// bridge.registerTrigger({
//   triggerType: 'every-10-seconds',
//   functionPath: 'engine.echo',
//   config: { name: 'EngineEcho', args: { text: 'Hello, Engine!', from: 'typescript-example' } },
// })
