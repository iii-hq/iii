import { beforeAll, beforeEach, describe, expect, it } from 'vitest'
import { registerWorker } from '../src/index'
import { iii, sleep } from './utils'

const EW_URL = process.env.III_RBAC_WORKER_URL ?? 'ws://localhost:49135'

type AuthInput = {
  headers: Record<string, string>
  query_params: Record<string, string[]>
}

// Invocation tracking
let authCalls: AuthInput[] = []

beforeAll(async () => {
  // Auth function – delegates to the mutable authHandler
  iii.registerFunction({ id: 'test::rbac-worker::auth' }, async (input: AuthInput) => {
    authCalls.push(input)
    const token = input.headers?.['x-test-token']

    if (!token) {
      return {
        allowed_functions: [],
        forbidden_functions: [],
        context: { role: 'anonymous', user_id: 'anonymous' },
      }
    }

    if (token === 'valid-token') {
      return {
        allowed_functions: ['test::ew::valid-token-echo'],
        forbidden_functions: [],
        context: { role: 'admin', user_id: 'user-1' },
      }
    }

    if (token === 'restricted-token') {
      return {
        allowed_functions: [],
        forbidden_functions: ['test::ew::echo'],
        context: { role: 'restricted', user_id: 'user-2' },
      }
    }

    throw new Error('invalid token')
  })

  // Middleware function – delegates to the mutable middlewareHandler
  iii.registerFunction({ id: 'test::rbac-worker::middleware' }, async (input: Record<string, unknown>) => {
    const functionId = input.function_id as string
    const payload = input.payload as Record<string, unknown>
    const context = input.context as Record<string, unknown>

    const enrichedPayload = { ...payload, _intercepted: true, _caller: context.user_id }
    return iii.trigger({ function_id: functionId, payload: enrichedPayload })
  })

  // Exposed via match("test::ew::*")
  iii.registerFunction({ id: 'test::ew::public::echo' }, async (data: Record<string, unknown>) => {
    return { echoed: data }
  })

  iii.registerFunction({ id: 'test::ew::valid-token-echo' }, async (data: Record<string, unknown>) => {
    return { echoed: data, valid_token: true }
  })

  // Exposed via metadata filter { ew_public: true }
  iii.registerFunction(
    { id: 'test::ew::meta-public', metadata: { ew_public: true } },
    async (data: Record<string, unknown>) => {
      return { meta_echoed: data }
    },
  )

  // NOT exposed – no match in expose_functions config
  iii.registerFunction({ id: 'test::ew::private' }, async () => {
    return { private: true }
  })

  await sleep(1000)
})

beforeEach(() => {
  authCalls = []
})

describe('RBAC Workers', () => {
  it('should return auth result for valid token', async () => {
    const iiiClient = registerWorker(EW_URL, {
      headers: { 'x-test-token': 'valid-token' },
      otel: { enabled: false },
    })

    try {
      // biome-ignore lint/suspicious/noExplicitAny: any is fine here
      const result = await iiiClient.trigger<any, any>({
        function_id: 'test::ew::valid-token-echo',
        payload: { msg: 'hello' },
      })

      expect(result.valid_token).toBe(true)
      expect(result.echoed.msg).toBe('hello')
      expect(result.echoed._caller).toBe('user-1')

      expect(authCalls).toHaveLength(1)
      expect(authCalls[0].headers['x-test-token']).toBe('valid-token')
    } finally {
      await iiiClient.shutdown()
    }
  })

  it('should return error for private function', async () => {
    const iiiClient = registerWorker(EW_URL, {
      headers: { 'x-test-token': 'valid-token' },
      otel: { enabled: false },
    })

    try {
      await expect(
        // biome-ignore lint/suspicious/noExplicitAny: any is fine here
        iiiClient.trigger<any, any>({
          function_id: 'test::ew::private',
          payload: { msg: 'hello' },
        }),
      ).rejects.toThrow()
    } finally {
      await iiiClient.shutdown()
    }
  })

  it('should return forbidden_functions for restricted token', async () => {
    const iiiClient = registerWorker(EW_URL, {
      headers: { 'x-test-token': 'restricted-token' },
      otel: { enabled: false },
    })

    try {
      await expect(
        // biome-ignore lint/suspicious/noExplicitAny: any is fine here
        iiiClient.trigger<any, any>({
          function_id: 'test::ew::echo',
          payload: { msg: 'hello' },
        }),
      ).rejects.toThrow()
    } finally {
      await iiiClient.shutdown()
    }
  })
})
