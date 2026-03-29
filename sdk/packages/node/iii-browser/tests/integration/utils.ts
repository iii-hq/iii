import { WebSocket } from 'ws'

// Must be set before registerWorker is called since it uses `new WebSocket()`
// at construction time.
globalThis.WebSocket = WebSocket as unknown as typeof globalThis.WebSocket

import { registerWorker } from '../../src/index'

const ENGINE_WS_URL = process.env.III_URL ?? 'ws://localhost:49199'
const RETRY_LIMIT = 100
const DELAY_MS = 100

export const engineWsUrl = ENGINE_WS_URL

export const iii = registerWorker(engineWsUrl, {
  reconnectionConfig: {
    maxRetries: 3,
    initialDelayMs: 100,
    maxDelayMs: 1000,
  },
})

export function sleep(duration: number): Promise<void> {
  return new Promise((resolve) => {
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
