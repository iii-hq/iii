import { describe, expect, it } from 'vitest'
import { CallAction, ChannelReader, ChannelWriter, registerWorker, TriggerAction } from '../src/index'
import type { UpdateAppend, UpdateOp } from '../src/stream'
import { MockEngine } from './mock-websocket'

describe('Package Exports', () => {
  it('should export main SDK symbols', () => {
    expect(registerWorker).toBeDefined()
    expect(typeof registerWorker).toBe('function')

    expect(CallAction).toBeDefined()
    expect(CallAction.Enqueue({ queue: 'orders' })).toEqual({ type: 'enqueue', queue: 'orders' })
    expect(CallAction.Void()).toEqual({ type: 'void' })
    expect(TriggerAction).toBe(CallAction)

    expect(ChannelReader).toBeDefined()
    expect(typeof ChannelReader).toBe('function')

    expect(ChannelWriter).toBeDefined()
    expect(typeof ChannelWriter).toBe('function')
  })

  it('registerWorker returns an ISdk with call, its trigger alias and the observer hooks', async () => {
    const engine = new MockEngine({ autoOpen: false })
    engine.install()
    const sdk = registerWorker('ws://exports.test')
    try {
      expect(typeof sdk.call).toBe('function')
      expect(sdk.trigger).toBe(sdk.call)
      for (const method of [
        'onObserverAdded',
        'onObserverRemoved',
        'onObserverMatch',
        'onObserverDeliver',
        'observers',
        'isObserved',
        'observe',
        'withInvocation',
      ]) {
        expect(typeof (sdk as unknown as Record<string, unknown>)[method]).toBe('function')
      }
      for (const removed of ['registerTrigger', 'registerTriggerType', 'unregisterTriggerType']) {
        expect((sdk as unknown as Record<string, unknown>)[removed]).toBeUndefined()
      }
    } finally {
      await sdk.shutdown()
      engine.uninstall()
    }
  })

  it('exports CallAction, ObserveRejection and EngineSources from the root, and no trigger API', async () => {
    const root = (await import('../src/index')) as Record<string, unknown>
    expect(root.TriggerAction).toBe(root.CallAction)
    expect(typeof root.ObserveRejection).toBe('function')
    expect(root.EngineSources).toMatchObject({
      FUNCTIONS_CHANGED: 'engine::on-functions-changed',
      WORKERS_CHANGED: 'engine::on-workers-changed',
      OBSERVERS_CHANGED: 'engine::on-observers-changed',
      LOG: 'observability::on-log',
      SPAN: 'observability::on-span',
      CONFIGURATION_UPDATED: 'configuration::on-updated',
      STREAM_CHANGED: 'stream::on-changed',
      STREAM_JOIN: 'stream::on-join',
      STREAM_LEAVE: 'stream::on-leave',
    })
    expect(root.EngineTriggers).toBeUndefined()
    expect(Object.values(root.EngineFunctions as Record<string, string>)).not.toContainEqual(
      expect.stringContaining('trigger'),
    )
  })

  it('should import stream module', async () => {
    const streamModule = await import('../src/stream')
    expect(streamModule).toBeDefined()
    // stream.ts exports only types (IStream, StreamGetInput, etc.) which are
    // erased at runtime, so we just verify the module resolves successfully
  })

  it('should type append as a browser update operation', () => {
    const op = { type: 'append', path: 'chunks', value: 'hello' } satisfies UpdateAppend
    const ops: UpdateOp[] = [op]

    expect(ops[0]).toEqual({ type: 'append', path: 'chunks', value: 'hello' })
  })

  it('should type append with nested array path (issue #1552 case 3)', () => {
    // Closes issue #1552: array-form path is the new happy path. The
    // TS shape mirrors UpdateMerge's MergePath = string | string[].
    const op = {
      type: 'append',
      path: ['entityId', 'buffer'],
      value: 'chunk',
    } satisfies UpdateAppend
    const ops: UpdateOp[] = [op]

    expect(ops[0]).toEqual({
      type: 'append',
      path: ['entityId', 'buffer'],
      value: 'chunk',
    })
  })

  it('should type append with omitted path (root append)', () => {
    // Path is now optional — omitted, empty string, and empty array all
    // route to root append in the engine.
    const op = { type: 'append', value: 'first' } satisfies UpdateAppend
    const ops: UpdateOp[] = [op]

    expect(ops[0]).toEqual({ type: 'append', value: 'first' })
  })

  it('should import state module', async () => {
    const stateModule = await import('../src/state')
    expect(stateModule).toBeDefined()
    expect(stateModule.StateEventType).toBeDefined()
    expect(Object.keys(stateModule).length).toBeGreaterThan(0)
  })

  it('should not have telemetry exports', async () => {
    const indexExports = await import('../src/index')
    const exportKeys = Object.keys(indexExports)

    expect(exportKeys).not.toContain('initOtel')
    expect(exportKeys).not.toContain('shutdownOtel')
    expect(exportKeys).not.toContain('getTracer')
    expect(exportKeys).not.toContain('getMeter')
    expect(exportKeys).not.toContain('isInitialized')
    expect(exportKeys).not.toContain('SpanKind')
    expect(exportKeys).not.toContain('SpanStatus')
    expect(exportKeys).not.toContain('SpanStatusCode')
  })
})
