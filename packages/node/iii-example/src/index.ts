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
}

const engine = new Engine()

// Not quite necessary to do it
bridge.registerService({ id: 'engine', description: 'Example of an engine service' })

bridge.registerFunction({ functionPath: 'engine.enable', description: 'Enable the engine' }, async () =>
  engine.enable(),
)
bridge.registerFunction({ functionPath: 'engine.disable', description: 'Disable the engine' }, async () =>
  engine.disable(),
)
bridge.registerFunction({ functionPath: 'engine.work', description: 'Work the engine' }, async () => engine.work())

setInterval(() => {
  logger.info('Working...')
}, 1000)
