import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { registerWorker } from '../src/iii'
import type { ISdk } from '../src/types'
import { MockEngine } from './mock-websocket'

/**
 * A worker's namespace is inherited by what it calls and what it registers.
 *
 * Wire-level: each assertion is the frame the SDK actually emits, so these hold
 * without an engine.
 *
 * The browser SDK was left behind when the other SDKs adopted the rule. Its
 * typed helper inherited, so the shape looked right; the low-level
 * `registerTrigger` and `trigger` did not, and those are the paths a worker
 * uses. A browser worker in `orders` therefore called into `default` and
 * registered triggers that fired and resolved nothing -- registration
 * succeeded, the trigger existed, it fired on time, and nothing happened.
 */
describe('namespace inheritance', () => {
  let engine: MockEngine
  let sdk: ISdk

  beforeEach(() => {
    engine = new MockEngine()
    engine.install()
  })

  afterEach(async () => {
    await sdk.shutdown()
    engine.uninstall()
  })

  const connect = async (namespace?: string): Promise<void> => {
    sdk = registerWorker('ws://test:49135', namespace ? { namespace } : {})
    await engine.waitForOpen()
  }

  it('registers a trigger in the worker declared namespace', async () => {
    await connect('orders')
    sdk.registerTrigger({ type: 'cron', function_id: 'api::process', config: {} })

    expect(engine.findSent('registertrigger')).toMatchObject({ namespace: 'orders' })
  })

  it('lets an explicit namespace win', async () => {
    // Naming another namespace, `default` included, is how a worker inside one
    // reaches an engine builtin.
    await connect('orders')
    sdk.registerTrigger({
      type: 'cron',
      function_id: 'state::sweep',
      config: {},
      namespace: 'default',
    })

    expect(engine.findSent('registertrigger')).toMatchObject({ namespace: 'default' })
  })

  it('leaves a worker without a namespace unchanged', async () => {
    await connect()
    sdk.registerTrigger({ type: 'cron', function_id: 'api::process', config: {} })

    expect(engine.findSent('registertrigger')).not.toHaveProperty('namespace')
  })

  it('keeps an invocation in the caller namespace', async () => {
    await connect('orders')
    void sdk.trigger({ function_id: 'api::ping', payload: {}, action: { type: 'void' } })

    const call = engine
      .findAllSent('invokefunction')
      .find((f) => f.function_id === 'api::ping')
    expect(call).toMatchObject({ namespace: 'orders' })
  })

  it('keeps a non-void invocation in the caller namespace too', async () => {
    // The two branches of `trigger` send separately, so both are asserted: the
    // void one returns immediately, this one waits on a reply.
    await connect('orders')
    void sdk.trigger({ function_id: 'api::ask', payload: {} }).catch(() => {})

    const call = engine.findAllSent('invokefunction').find((f) => f.function_id === 'api::ask')
    expect(call).toMatchObject({ namespace: 'orders' })
  })

  it('keeps an implicit engine invocation in default', async () => {
    await connect('orders')
    void sdk.trigger({
      function_id: 'engine::channels::create',
      payload: {},
      action: { type: 'void' },
    })

    const call = engine.findAllSent('invokefunction').find((f) => f.function_id === 'engine::channels::create')
    expect(call).toMatchObject({ namespace: 'default' })
  })

  it('lets an explicit namespace win for an engine invocation', async () => {
    await connect('orders')
    void sdk.trigger({
      function_id: 'engine::channels::create',
      payload: {},
      action: { type: 'void' },
      namespace: 'sandbox',
    })

    const call = engine.findAllSent('invokefunction').find((f) => f.function_id === 'engine::channels::create')
    expect(call).toMatchObject({ namespace: 'sandbox' })
  })

  /**
   * The regression the Node SDK hit when it adopted this rule.
   *
   * The SDK announces itself with `engine::workers::register`, and it does so
   * through the same call path. Once that path inherited, the announcement
   * followed the worker into its own namespace -- where the engine does not
   * serve that function -- so the worker never registered and every function it
   * offered looked missing.
   */
  it('never redirects the workers own registration', async () => {
    await connect('orders')

    const announce = engine
      .findAllSent('invokefunction')
      .find((f) => f.function_id === 'engine::workers::register')
    expect(announce, 'the worker announces itself').toBeDefined()
    expect(announce).not.toHaveProperty('namespace')
    // The namespace still travels, as data the engine files the worker under.
    expect((announce as { data?: { namespace?: string } }).data?.namespace).toBe('orders')
  })
})

/**
 * A namespace that was declared and left blank is a mistake, not a way to ask
 * for `default`.
 *
 * The browser assigned it raw, so `namespace: ''` produced a worker that
 * registered under a namespace named by the empty string -- one nobody can
 * address or type. The other SDKs each refused it already, in their own way.
 */
describe('a blank namespace is refused', () => {
  let engine: MockEngine

  beforeEach(() => {
    engine = new MockEngine()
    engine.install()
  })

  afterEach(() => {
    engine.uninstall()
  })

  it.each([['', 'empty'], ['   ', 'whitespace-only']])(
    'refuses a %s namespace',
    (namespace) => {
      expect(() => registerWorker('ws://test:49135', { namespace })).toThrow(/namespace is empty/)
    },
  )

  it('says what to do instead', () => {
    expect(() => registerWorker('ws://test:49135', { namespace: '' })).toThrow(
      /leave it unset to register in `default`/,
    )
  })

  it('leaves an absent namespace alone', () => {
    // The control: absent asks for the engine's `default` and is not a mistake.
    expect(() => registerWorker('ws://test:49135', {})).not.toThrow()
  })
})

/**
 * The same rule on the per-call path.
 *
 * `??` forwards the empty string, Python's `or` replaced it with the worker's
 * and Go dropped it -- one mistake, four behaviours, none of them chosen. Each
 * SDK now refuses it where it is written.
 */
describe('a blank namespace on one call is refused', () => {
  let engine: MockEngine
  let sdk: ISdk

  beforeEach(async () => {
    engine = new MockEngine()
    engine.install()
    sdk = registerWorker('ws://test:49135', { namespace: 'orders' })
    await engine.waitForOpen()
  })

  afterEach(async () => {
    await sdk.shutdown()
    engine.uninstall()
  })

  it('refuses it on registerTrigger', () => {
    expect(() =>
      sdk.registerTrigger({ type: 'cron', function_id: 'api::process', config: {}, namespace: '' }),
    ).toThrow(/namespace is empty/)
  })

  it('refuses it on trigger', async () => {
    await expect(
      sdk.trigger({ function_id: 'api::ping', payload: {}, namespace: '   ' }),
    ).rejects.toThrow(/namespace is empty/)
  })

  it('still inherits when absent', () => {
    // The control: absent is not blank, and still means this worker's.
    sdk.registerTrigger({ type: 'cron', function_id: 'api::process', config: {} })
    expect(engine.findSent('registertrigger')).toMatchObject({ namespace: 'orders' })
  })
})
