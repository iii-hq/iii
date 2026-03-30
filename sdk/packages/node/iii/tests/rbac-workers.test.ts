import { beforeAll, beforeEach, describe, expect, it } from 'vitest'
import type {
  AuthInput,
  AuthResult,
  MiddlewareFunctionInput,
  OnFunctionRegistrationInput,
  OnTriggerRegistrationInput,
  OnTriggerTypeRegistrationInput,
} from '../src/index'
import { registerWorker } from '../src/index'
import { iii, sleep } from './utils'

const EW_URL = process.env.III_RBAC_WORKER_URL ?? 'ws://localhost:49135'

let authCalls: AuthInput[] = []
let triggerTypeRegCalls: OnTriggerTypeRegistrationInput[] = []
let triggerRegCalls: OnTriggerRegistrationInput[] = []

beforeAll(async () => {
  iii.registerFunction({ id: 'test::rbac-worker::auth' }, async (input: AuthInput): Promise<AuthResult> => {
    authCalls.push(input)
    const token = input.headers?.['x-test-token']

    if (!token) {
      return {
        allowed_functions: [],
        forbidden_functions: [],
        allow_trigger_type_registration: false,
        context: { role: 'anonymous', user_id: 'anonymous' },
      }
    }

    if (token === 'valid-token') {
      return {
        allowed_functions: ['test::ew::valid-token-echo'],
        forbidden_functions: [],
        allow_trigger_type_registration: true,
        context: { role: 'admin', user_id: 'user-1' },
      }
    }

    if (token === 'restricted-token') {
      return {
        allowed_functions: [],
        forbidden_functions: ['test::ew::echo'],
        allow_trigger_type_registration: false,
        context: { role: 'restricted', user_id: 'user-2' },
      }
    }

    throw new Error('invalid token')
  })

  iii.registerFunction({ id: 'test::rbac-worker::middleware' }, async (input: MiddlewareFunctionInput) => {
    const enrichedPayload = { ...input.payload, _intercepted: true, _caller: input.context.user_id }
    return iii.trigger({ function_id: input.function_id, payload: enrichedPayload })
  })

  iii.registerFunction(
    { id: 'test::rbac-worker::on-function-reg' },
    async (input: OnFunctionRegistrationInput) => {
      return !input.function_id.startsWith('denied::')
    },
  )

  iii.registerFunction(
    { id: 'test::rbac-worker::on-trigger-type-reg' },
    async (input: OnTriggerTypeRegistrationInput) => {
      triggerTypeRegCalls.push(input)
      return !input.trigger_type_id.startsWith('denied-tt::')
    },
  )

  iii.registerFunction(
    { id: 'test::rbac-worker::on-trigger-reg' },
    async (input: OnTriggerRegistrationInput) => {
      triggerRegCalls.push(input)
      return !input.function_id.startsWith('denied-trig::')
    },
  )

  iii.registerTriggerType(
    { id: 'test-rbac-trigger', description: 'Trigger type for RBAC tests' },
    {
      async registerTrigger() {},
      async unregisterTrigger() {},
    },
  )

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
  triggerTypeRegCalls = []
  triggerRegCalls = []
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

  it('should deny trigger type registration via on_trigger_type_registration hook', async () => {
    const iiiClient = registerWorker(EW_URL, {
      headers: { 'x-test-token': 'valid-token' },
      otel: { enabled: false },
    })

    try {
      iiiClient.registerTriggerType(
        { id: 'denied-tt::test', description: 'Should be denied' },
        {
          async registerTrigger() {},
          async unregisterTrigger() {},
        },
      )

      await sleep(1000)

      expect(triggerTypeRegCalls).toHaveLength(1)
      expect(triggerTypeRegCalls[0].trigger_type_id).toBe('denied-tt::test')
      expect(triggerTypeRegCalls[0].description).toBe('Should be denied')
      expect(triggerTypeRegCalls[0].context.user_id).toBe('user-1')
    } finally {
      await iiiClient.shutdown()
    }
  })

  it('should deny trigger registration via on_trigger_registration hook', async () => {
    const iiiClient = registerWorker(EW_URL, {
      headers: { 'x-test-token': 'valid-token' },
      otel: { enabled: false },
    })

    try {
      iiiClient.registerTrigger({
        type: 'test-rbac-trigger',
        function_id: 'denied-trig::my-fn',
        config: { key: 'value' },
      })

      await sleep(1000)

      expect(triggerRegCalls).toHaveLength(1)
      expect(triggerRegCalls[0].trigger_type).toBe('test-rbac-trigger')
      expect(triggerRegCalls[0].function_id).toBe('denied-trig::my-fn')
      expect(triggerRegCalls[0].context.user_id).toBe('user-1')
    } finally {
      await iiiClient.shutdown()
    }
  })

  it('should deny function registration via on_function_registration hook', async () => {
    const iiiClient = registerWorker(EW_URL, {
      headers: { 'x-test-token': 'valid-token' },
      otel: { enabled: false },
    })

    try {
      iiiClient.registerFunction({ id: 'denied::blocked-fn' }, async () => {
        return { should: 'not reach' }
      })

      await sleep(1000)

      await expect(
        // biome-ignore lint/suspicious/noExplicitAny: any is fine here
        iiiClient.trigger<any, any>({
          function_id: 'denied::blocked-fn',
          payload: {},
        }),
      ).rejects.toThrow()
    } finally {
      await iiiClient.shutdown()
    }
  })
})
