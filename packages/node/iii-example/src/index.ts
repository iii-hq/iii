import { bridge } from './bridge'
import { logger } from './logger'

class Engine {
  enabled: boolean

  constructor() {
    this.enabled = false
  }

  enable() {
    this.enabled = true
    logger.info('Enabling Engine')
  }

  disable() {
    this.enabled = false
    logger.info('Disabling Engine')
  }

  work() {
    if (this.enabled) {
      logger.info('Working...')
    } else {
      logger.warn('Engine is disabled')
    }
  }

  echo(payload: { text: string; from?: string }) {
    const response = {
      text: payload.text,
      from: 'engine.echo',
      receivedFrom: payload.from ?? 'unknown',
      at: new Date().toISOString(),
    }
    logger.info('Echoing payload', response)
    return response
  }
}

const engine = new Engine()

// const noPayloadFormat = {
//   name: 'payload',
//   description: 'This function does not expect any input payload.',
//   type: 'null',
//   required: false,
// } as const

// const voidResponseFormat = {
//   name: 'response',
//   description: 'This function does not return a response payload.',
//   type: 'null',
//   required: false,
// } as const

// const echoRequestFormat = {
//   name: 'payload',
//   description: 'Payload forwarded to engine.echo.',
//   type: 'object',
//   body: [
//     {
//       name: 'text',
//       description: 'Message that will be echoed back.',
//       type: 'string',
//       required: true,
//     },
//     {
//       name: 'from',
//       description: 'Optional identifier for the caller.',
//       type: 'string',
//       required: false,
//     },
//   ],
// } as const

// const echoResponseFormat = {
//   name: 'response',
//   description: 'Metadata returned by engine.echo.',
//   type: 'object',
//   body: [
//     {
//       name: 'text',
//       description: 'The echoed text.',
//       type: 'string',
//       required: true,
//     },
//     {
//       name: 'from',
//       description: 'Identifier of the echo handler.',
//       type: 'string',
//       required: true,
//     },
//     {
//       name: 'receivedFrom',
//       description: 'Caller identifier embedded in the payload.',
//       type: 'string',
//       required: true,
//     },
//     {
//       name: 'at',
//       description: 'ISO-8601 timestamp when the request was processed.',
//       type: 'string',
//       required: true,
//     },
//   ],
// } as const

// Not quite necessary to do it
// bridge.registerService({ id: 'engine', description: 'Example of an engine service' })

bridge.registerFunction(
  {
    functionPath: 'engine.enable',
    description: 'Enable the engine',
  },
  async () => engine.enable(),
)
bridge.registerFunction(
  {
    functionPath: 'engine.disable',
    description: 'Disable the engine',
  },
  async () => engine.disable(),
)
bridge.registerFunction(
  {
    functionPath: 'engine.work',
    description: 'Work the engine',
  },
  async () => engine.work(),
)
bridge.registerFunction(
  {
    functionPath: 'engine.echo',
    description: 'Echo message back to the caller',
  },
  async (payload) => engine.echo(payload),
)

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
  functionPath: 'engine.echo',
  config: { topic: 'echo' },
})

process.stdin.on('data', async (data) => {
  await bridge.invokeFunction('emit', { topic: 'echo', data: { text: data.toString().trim() } })
  // bridge.invokeFunctionAsync('engine.echo', { text: data.toString().trim() })
  console.log('Event emitted')
})

// bridge.registerTrigger({
//   triggerType: 'every-10-seconds',
//   functionPath: 'engine.echo',
//   config: { name: 'EngineEcho', args: { text: 'Hello, Engine!', from: 'typescript-example' } },
// })
