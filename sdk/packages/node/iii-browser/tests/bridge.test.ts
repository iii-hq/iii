import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { registerWorker, TriggerAction } from '../src/iii'
import type { ISdk } from '../src/types'
import { MockEngine } from './mock-websocket'

describe('Bridge Operations', () => {
  let engine: MockEngine
  let sdk: ISdk

  beforeEach(async () => {
    engine = new MockEngine()
    engine.install()
    sdk = registerWorker('ws://test:49135')
    await engine.waitForOpen()
  })

  afterEach(async () => {
    await sdk.shutdown()
    engine.uninstall()
  })

  it('should connect to the correct URL', () => {
    expect(engine.socket.url).toBe('ws://test:49135')
  })

  it('should send registerfunction message', () => {
    sdk.registerFunction('test.echo', async (data) => ({ echoed: data }))

    const msg = engine.findSent('registerfunction')
    expect(msg).toBeDefined()
    expect(msg?.id).toBe('test.echo')
  })

  it('should throw on empty function id', () => {
    expect(() => sdk.registerFunction('', async () => ({}))).toThrow('id is required')
  })

  it('should throw on duplicate function id', () => {
    sdk.registerFunction('test.dup', async () => ({}))
    expect(() => sdk.registerFunction('test.dup', async () => ({}))).toThrow('function id already registered: test.dup')
  })

  it('should trigger sync invocation and resolve on result', async () => {
    const triggerPromise = sdk.trigger<{ msg: string }, { echoed: { msg: string } }>({
      function_id: 'remote.echo',
      payload: { msg: 'hello' },
    })

    await new Promise<void>((r) => queueMicrotask(r))

    const invokeMsgs = engine.findAllSent('invokefunction')
    const invokeMsg = invokeMsgs.find((m) => m.function_id === 'remote.echo')
    expect(invokeMsg).toBeDefined()
    expect(invokeMsg?.data).toEqual({ msg: 'hello' })

    engine.respondToInvocation(invokeMsg?.invocation_id as string, { echoed: { msg: 'hello' } })

    const result = await triggerPromise
    expect(result).toEqual({ echoed: { msg: 'hello' } })
  })

  it('should trigger void and return undefined', async () => {
    sdk.registerFunction('test.void-target', async () => ({}))

    const result = await sdk.trigger({
      function_id: 'test.void-target',
      payload: { value: 42 },
      action: TriggerAction.Void(),
    })

    expect(result).toBeUndefined()

    const invokeMsg = engine.findSent('invokefunction')
    expect(invokeMsg).toBeDefined()
    expect(invokeMsg?.action).toEqual({ type: 'void' })
    expect(invokeMsg?.invocation_id).toBeUndefined()
  })

  it('should trigger enqueue and resolve with receipt', async () => {
    const triggerPromise = sdk.trigger({
      function_id: 'queue.job',
      payload: { task: 'process' },
      action: TriggerAction.Enqueue({ queue: 'work' }),
    })

    await new Promise<void>((r) => queueMicrotask(r))

    const invokeMsgs = engine.findAllSent('invokefunction')
    const invokeMsg = invokeMsgs.find((m) => m.function_id === 'queue.job')
    expect(invokeMsg).toBeDefined()
    expect(invokeMsg?.action).toEqual({ type: 'enqueue', queue: 'work' })

    engine.respondToInvocation(invokeMsg?.invocation_id as string, { messageReceiptId: 'receipt-123' })

    const result = await triggerPromise
    expect(result).toEqual({ messageReceiptId: 'receipt-123' })
  })

  it('should reject trigger on timeout', async () => {
    const triggerPromise = sdk.trigger({
      function_id: 'slow.function',
      payload: {},
      timeoutMs: 50,
    })

    await expect(triggerPromise).rejects.toThrow('Invocation timeout after 50ms: slow.function')

    // Absorb the shutdown rejection that would otherwise be unhandled
    triggerPromise.catch(() => {
      //
    })
  })

  it('should handle engine-initiated function invocation', async () => {
    let receivedData: unknown
    sdk.registerFunction('test.handler', async (data) => {
      receivedData = data
      return { processed: true }
    })

    const invocationId = crypto.randomUUID()
    engine.invokeFunction('test.handler', { key: 'value' }, invocationId)

    await new Promise<void>((r) => setTimeout(r, 10))

    expect(receivedData).toEqual({ key: 'value' })

    const resultMsg = engine.findSent('invocationresult')
    expect(resultMsg).toBeDefined()
    expect(resultMsg?.invocation_id).toBe(invocationId)
    expect(resultMsg?.result).toEqual({ processed: true })
    expect(resultMsg?.error).toBeUndefined()
  })

  it('should send error result when handler throws', async () => {
    sdk.registerFunction('test.failing', async () => {
      throw new Error('handler exploded')
    })

    const invocationId = crypto.randomUUID()
    engine.invokeFunction('test.failing', {}, invocationId)

    await new Promise<void>((r) => setTimeout(r, 10))

    const resultMsg = engine.findSent('invocationresult')
    expect(resultMsg).toBeDefined()
    expect(resultMsg?.invocation_id).toBe(invocationId)
    expect(resultMsg?.error).toBeDefined()
    const error = resultMsg?.error as Record<string, unknown>
    expect(error.code).toBe('invocation_failed')
    expect(error.message).toBe('handler exploded')
  })

  it('should send error for non-existent function invocation', async () => {
    const invocationId = crypto.randomUUID()
    engine.invokeFunction('does.not.exist', {}, invocationId)

    await new Promise<void>((r) => setTimeout(r, 10))

    const resultMsg = engine.findSent('invocationresult')
    expect(resultMsg).toBeDefined()
    const error = resultMsg?.error as Record<string, unknown>
    expect(error.code).toBe('function_not_found')
  })

  it('should send unregister message on unregister', () => {
    const fn = sdk.registerFunction('test.removable', async () => ({}))
    fn.unregister()

    const msg = engine.findSent('unregisterfunction')
    expect(msg).toBeDefined()
    expect(msg?.id).toBe('test.removable')
  })

  it('should close WS and reject pending invocations on shutdown', async () => {
    const triggerPromise = sdk.trigger({
      function_id: 'remote.fn',
      payload: {},
    })

    await new Promise<void>((r) => queueMicrotask(r))

    await sdk.shutdown()

    await expect(triggerPromise).rejects.toThrow('iii is shutting down')
  })

  it('should re-register functions and triggers on reconnect', async () => {
    sdk.registerFunction('test.persist', async () => ({ ok: true }))
    sdk.registerTrigger({
      type: 'cron',
      function_id: 'test.persist',
      config: { expression: '* * * * *' },
    })

    const firstSocket = engine.socket

    firstSocket.simulateClose()

    // Wait for reconnection (initialDelayMs=1000 + up to 30% jitter)
    let secondSocket = engine.sockets[engine.sockets.length - 1]
    const deadline = Date.now() + 3000
    while (secondSocket === firstSocket && Date.now() < deadline) {
      await new Promise<void>((r) => setTimeout(r, 100))
      secondSocket = engine.sockets[engine.sockets.length - 1]
    }
    expect(secondSocket).not.toBe(firstSocket)

    secondSocket.simulateOpen()
    await new Promise<void>((r) => queueMicrotask(r))

    const reRegistered = secondSocket.findAllSent('registerfunction')
    expect(reRegistered.length).toBeGreaterThanOrEqual(1)
    expect(reRegistered.some((m) => m.id === 'test.persist')).toBe(true)

    const reRegisteredTriggers = secondSocket.findAllSent('registertrigger')
    expect(reRegisteredTriggers.length).toBeGreaterThanOrEqual(1)
  })
})
