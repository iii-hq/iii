import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { type WebSocket, WebSocketServer } from 'ws'
import { registerWorker } from '../src/iii'
import type { IIIClient } from '../src/types'

/**
 * A worker's namespace is inherited by what it calls and what it registers.
 *
 * Wire-level: the assertion is the frame the SDK actually emits, so these hold
 * without an engine.
 *
 * Without the rule, calls and observers resolved in the engine's `default`
 * namespace, so a worker in `orders` called into a namespace it did not live
 * in and created observers that named functions it never registered there.
 */
describe('namespace inheritance', () => {
  let wss: WebSocketServer
  let url: string
  let sdk: IIIClient | undefined
  let frames: Record<string, unknown>[]

  beforeEach(async () => {
    frames = []
    wss = new WebSocketServer({ port: 0 })
    await new Promise<void>((resolve) => wss.once('listening', () => resolve()))
    url = `ws://127.0.0.1:${(wss.address() as { port: number }).port}`
    wss.on('connection', (ws: WebSocket) => {
      ws.send(
        JSON.stringify({ type: 'workerregistered', worker_id: 'test-worker', features: ['observe.v1', 'observe.v2'] }),
      )
      ws.on('message', (raw) => {
        try {
          frames.push(JSON.parse(String(raw)))
        } catch {
          /* not our frame */
        }
      })
    })
  })

  afterEach(async () => {
    if (sdk) await sdk.shutdown()
    sdk = undefined
    await new Promise<void>((resolve) => wss.close(() => resolve()))
  })

  const settle = (ms = 300) => new Promise((r) => setTimeout(r, ms))
  const firstOf = (type: string) => frames.find((f) => f.type === type)

  it('observes in the worker declared namespace', async () => {
    sdk = registerWorker(url, { workerName: 'tester', namespace: 'orders', otel: { enabled: false } })
    await settle()
    sdk.observe('api::process', 'api::on-process')
    await settle()

    expect(firstOf('observe')).toMatchObject({
      source: { function_id: 'api::process', namespace: 'orders' },
      target: { function_id: 'api::on-process', namespace: 'orders' },
    })
  })

  it('lets an explicit namespace win', async () => {
    sdk = registerWorker(url, { workerName: 'tester', namespace: 'orders', otel: { enabled: false } })
    await settle()
    sdk.observe(
      { function_id: 'state::sweep', namespace: 'default' },
      { function_id: 'api::on-sweep', namespace: 'ops' },
    )
    await settle()

    expect(firstOf('observe')).toMatchObject({
      source: { function_id: 'state::sweep', namespace: 'default' },
      target: { function_id: 'api::on-sweep', namespace: 'ops' },
    })
  })

  it('leaves a worker without a namespace unchanged', async () => {
    sdk = registerWorker(url, { workerName: 'tester', otel: { enabled: false } })
    await settle()
    sdk.observe('api::process', 'api::on-process')
    await settle()

    expect(firstOf('observe')).toEqual({
      type: 'observe',
      id: expect.any(String),
      source: { function_id: 'api::process' },
      target: { function_id: 'api::on-process' },
    })
  })

  it('keeps an invocation in the caller namespace', async () => {
    sdk = registerWorker(url, { workerName: 'tester', namespace: 'orders', otel: { enabled: false } })
    await settle()
    void sdk.call({ function_id: 'api::ping', payload: {}, action: { type: 'void' } } as never)
    await settle()

    const call = frames.find((f) => f.type === 'invokefunction' && f.function_id === 'api::ping')
    expect(call).toMatchObject({ namespace: 'orders' })
  })

  it('keeps an implicit engine invocation in default', async () => {
    sdk = registerWorker(url, { workerName: 'tester', namespace: 'orders', otel: { enabled: false } })
    await settle()
    void sdk.call({
      function_id: 'engine::channels::create',
      payload: {},
      action: { type: 'void' },
    } as never)
    await settle()

    const call = frames.find((f) => f.type === 'invokefunction' && f.function_id === 'engine::channels::create')
    expect(call).toMatchObject({ namespace: 'default' })
  })

  it('lets an explicit namespace win for an engine invocation', async () => {
    sdk = registerWorker(url, { workerName: 'tester', namespace: 'orders', otel: { enabled: false } })
    await settle()
    void sdk.call({
      function_id: 'engine::channels::create',
      payload: {},
      action: { type: 'void' },
      namespace: 'sandbox',
    } as never)
    await settle()

    const call = frames.find((f) => f.type === 'invokefunction' && f.function_id === 'engine::channels::create')
    expect(call).toMatchObject({ namespace: 'sandbox' })
  })

  it('keeps an implicit configuration invocation in default', async () => {
    sdk = registerWorker(url, { workerName: 'tester', namespace: 'orders', otel: { enabled: false } })
    await settle()
    void sdk.call({ function_id: 'configuration::get', payload: {}, action: { type: 'void' } } as never)
    await settle()

    const call = frames.find((f) => f.type === 'invokefunction' && f.function_id === 'configuration::get')
    expect(call).toMatchObject({ namespace: 'default' })
  })

  it('lets an explicit namespace win for a configuration invocation', async () => {
    sdk = registerWorker(url, { workerName: 'tester', namespace: 'orders', otel: { enabled: false } })
    await settle()
    void sdk.call({
      function_id: 'configuration::get',
      payload: {},
      action: { type: 'void' },
      namespace: 'sandbox',
    } as never)
    await settle()

    const call = frames.find((f) => f.type === 'invokefunction' && f.function_id === 'configuration::get')
    expect(call).toMatchObject({ namespace: 'sandbox' })
  })

  /**
   * The regression this file exists for.
   *
   * The SDK announces itself with `engine::workers::register`, and it does so
   * through `call`. The generic engine-builtin rule must keep the
   * announcement in `default`; callers must not need a one-off override.
   */
  it('never redirects the workers own registration', async () => {
    sdk = registerWorker(url, { workerName: 'tester', namespace: 'orders', otel: { enabled: false } })
    await settle(600)

    const announce = frames.find((f) => f.type === 'invokefunction' && f.function_id === 'engine::workers::register')
    expect(announce, 'the worker announces itself').toBeDefined()
    expect(announce).toMatchObject({ namespace: 'default' })
    // The namespace still travels, as data the engine files the worker under.
    expect((announce as { data?: { namespace?: string } }).data?.namespace).toBe('orders')
  })

  /**
   * A namespace that was declared and left blank is a mistake, not a way to ask
   * for `default`.
   *
   * Absent and blank mean opposite things, and read as the same they produce
   * the failure nobody can see: the worker registers in `default`, and since a
   * worker's calls and observers follow its namespace, the whole project
   * serves from a place the declaration never named.
   */
  describe('a blank namespace is refused', () => {
    afterEach(() => vi.unstubAllEnvs())

    it('rejects an empty option', () => {
      expect(() => registerWorker(url, { workerName: 'tester', namespace: '' })).toThrow(/namespace is empty/)
    })

    it('rejects a whitespace-only option', () => {
      expect(() => registerWorker(url, { workerName: 'tester', namespace: '   ' })).toThrow(/namespace is empty/)
    })

    // `FOO=` is how a shell says "not set", so a blank env var is read as
    // absent rather than refused. Absent is a namespace a worker may
    // legitimately have none of; an option written and left empty is not.
    it('leaves a blank III_NAMESPACE alone', () => {
      vi.stubEnv('III_NAMESPACE', '')
      expect(() => {
        sdk = registerWorker(url, { workerName: 'tester', otel: { enabled: false } })
      }).not.toThrow()
    })

    it('still accepts an unset III_NAMESPACE', () => {
      vi.stubEnv('III_NAMESPACE', undefined as unknown as string)
      expect(() => {
        sdk = registerWorker(url, { workerName: 'tester', otel: { enabled: false } })
      }).not.toThrow()
    })
  })
})
