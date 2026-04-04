// import { iii } from 'iii-sdk'
import { registerWorker, Logger } from '../src/index'

const ENGINE_WS_URL = process.env.III_URL ?? 'ws://localhost:49199'
const ENGINE_HTTP_URL = process.env.III_HTTP_URL ?? 'http://localhost:3199'
const RETRY_LIMIT = 100
const DELAY_MS = 100

export const engineWsUrl = ENGINE_WS_URL
export const engineHttpUrl = ENGINE_HTTP_URL

export const iii = registerWorker(engineWsUrl, {
  otel: {
    serviceName: 'iii-tests',
    serviceVersion: '0.0.1',
    reconnectionConfig: {
      maxRetries: 3,
      initialDelayMs: 100,
      maxDelayMs: 1000,
    },
  },
  reconnectionConfig: {
    maxRetries: 3,
    initialDelayMs: 100,
    maxDelayMs: 1000,
  },
})

// Standalone logger for test utilities — no trace context needed
export const logger = new Logger()

export async function httpRequest(
  method: string,
  path: string,
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  body?: any,
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
): Promise<{ status: number; data: any }> {
  const url = `${engineHttpUrl}${path}`
  const options: RequestInit = { method, headers: { 'Content-Type': 'application/json' } }

  if (body) {
    options.body = JSON.stringify(body)
  }

  const response = await fetch(url, options)
  const data = await response.json().catch(() => ({}))

  return { status: response.status, data }
}

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
