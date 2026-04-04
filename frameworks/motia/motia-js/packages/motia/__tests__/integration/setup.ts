const TEST_ENGINE_PORT = parseInt(process.env.TEST_ENGINE_PORT ?? '49199', 10)
const TEST_API_PORT = parseInt(process.env.TEST_API_PORT ?? '3199', 10)

export const TEST_ENGINE_URL = `ws://localhost:${TEST_ENGINE_PORT}`
export const TEST_API_URL = `http://localhost:${TEST_API_PORT}`

export const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms))

export function initTestEnv(): void {
  process.env.III_URL = process.env.III_URL ?? TEST_ENGINE_URL
}

export async function waitForReady(sdk: { trigger: (req: { function_id: string; payload: unknown }) => Promise<unknown> }): Promise<void> {
  const deadline = Date.now() + 13000
  const callTimeout = 300
  let delay = 50
  while (Date.now() < deadline) {
    try {
      await Promise.race([
        sdk.trigger({ function_id: 'engine::workers::list', payload: {} }),
        new Promise<never>((_, reject) => setTimeout(() => reject(new Error('call timeout')), callTimeout)),
      ])
      return
    } catch {
      await sleep(delay)
      delay = Math.min(delay * 2, 500)
    }
  }
  throw new Error('Engine not ready')
}

export async function waitForRegistration(
  sdk: { trigger: (req: { function_id: string; payload: unknown }) => Promise<unknown> },
  functionId: string,
  timeout = 5000,
): Promise<void> {
  const start = Date.now()
  const pollInterval = 100
  while (Date.now() - start < timeout) {
    try {
      const result = (await sdk.trigger({ function_id: 'engine::functions::list', payload: {} })) as {
        functions?: { function_id: string }[]
      }
      const ids = result?.functions?.map((f) => f.function_id) ?? []
      if (ids.includes(functionId)) return
    } catch {
      // engine not ready yet, keep polling
    }
    await sleep(pollInterval)
  }
  throw new Error(`Function ${functionId} was not registered within ${timeout}ms`)
}
