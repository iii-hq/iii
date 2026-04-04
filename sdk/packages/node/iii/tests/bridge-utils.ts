import { registerWorker, Logger } from '../src/index'

const BRIDGE_WS_URL = process.env.III_BRIDGE_WS_URL ?? 'ws://localhost:49197'
const RETRY_LIMIT = 100
const DELAY_MS = 100

export const bridgeIII = registerWorker(BRIDGE_WS_URL, {
  reconnectionConfig: {
    maxRetries: 3,
    initialDelayMs: 100,
    maxDelayMs: 1000,
  },
})

export const logger = new Logger()

export function sleep(duration: number): Promise<void> {
  return new Promise(resolve => {
    setTimeout(() => resolve(), duration)
  })
}

export async function execute<T>(operation: () => Promise<T>): Promise<T> {
  let currentAttempt = 0

  while (true) {
    try {
      return await operation()
    } catch (err) {
      currentAttempt++

      if (currentAttempt >= RETRY_LIMIT) {
        throw err
      }

      await sleep(DELAY_MS)
    }
  }
}
