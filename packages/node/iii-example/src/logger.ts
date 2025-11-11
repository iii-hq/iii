import { bridge } from './bridge'

export class Logger {
  info(message: string, data?: any) {
    bridge.invokeFunctionAsync('logger.info', { message, data })
    console.log(`[INFO] ${message}`, data)
  }

  warn(message: string, data?: any) {
    bridge.invokeFunctionAsync('logger.warn', { message, data })
    console.warn(`[WARN] ${message}`, data)
  }

  error(message: string, data?: any) {
    bridge.invokeFunctionAsync('logger.error', { message, data })
    console.error(`[ERROR] ${message}`, data)
  }
}

export const logger = new Logger()
