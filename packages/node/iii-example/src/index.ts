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

// Not quite necessary to do it
// bridge.registerService({ id: 'engine', description: 'Example of an engine service' })

bridge.registerFunction({ functionPath: 'engine.enable', description: 'Enable the engine' }, async () =>
  engine.enable(),
)
bridge.registerFunction({ functionPath: 'engine.disable', description: 'Disable the engine' }, async () =>
  engine.disable(),
)
bridge.registerFunction({ functionPath: 'engine.work', description: 'Work the engine' }, async () => engine.work())
bridge.registerFunction(
  { functionPath: 'engine.echo', description: 'Echo message back to the caller' },
  async (payload) => engine.echo(payload),
)

const handlers: Record<string, () => Promise<void>> = {}

setInterval(async () => {
  const handlersSize = Object.keys(handlers).length
  await Promise.allSettled(Object.values(handlers).map((handler) => handler()))
  console.log('Handlers executed (', handlersSize, ' handlers)')
}, 10000)

bridge.registerTriggerType<{ name: string }>(
  { id: 'every-10-seconds', description: 'Every 10 seconds trigger' },
  {
    registerTrigger: async ({ id, functionPath, config }) => {
      handlers[id] = async () => {
        await bridge.invokeFunction(functionPath, {})
        console.log(`Result of ${config.name} function in ${functionPath}`)
      }
    },
    unregisterTrigger: async ({ id }) => {
      delete handlers[id]
    },
  },
)

bridge.registerTrigger({
  id: crypto.randomUUID(),
  triggerType: 'every-10-seconds',
  functionPath: 'engine.echo',
  config: { name: 'EngineEcho' },
})
