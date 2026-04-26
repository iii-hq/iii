import { describe, expect, it } from 'vitest'
import { ChannelReader, ChannelWriter, registerWorker, TriggerAction } from '../src/index'
import type { UpdateAppend, UpdateMerge, UpdateOp } from '../src/stream'

describe('Package Exports', () => {
  it('should export main SDK symbols', () => {
    expect(registerWorker).toBeDefined()
    expect(typeof registerWorker).toBe('function')

    expect(TriggerAction).toBeDefined()
    expect(typeof TriggerAction.Enqueue).toBe('function')
    expect(typeof TriggerAction.Void).toBe('function')

    expect(ChannelReader).toBeDefined()
    expect(typeof ChannelReader).toBe('function')

    expect(ChannelWriter).toBeDefined()
    expect(typeof ChannelWriter).toBe('function')
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

  it('should type merge with a string path (legacy/first-level form)', () => {
    const op = {
      type: 'merge',
      path: 'session-abc',
      value: { author: 'alice' },
    } satisfies UpdateMerge

    expect(op).toEqual({ type: 'merge', path: 'session-abc', value: { author: 'alice' } })
  })

  it('should type merge with an array path (nested form)', () => {
    const op = {
      type: 'merge',
      path: ['sessions', 'abc'],
      value: { ts: 'chunk' },
    } satisfies UpdateMerge

    expect(op).toEqual({ type: 'merge', path: ['sessions', 'abc'], value: { ts: 'chunk' } })

    // Round-trips through JSON unchanged.
    const parsed: UpdateMerge = JSON.parse(JSON.stringify(op))
    expect(parsed).toEqual(op)
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
  })
})
