import { describe, expect, it } from 'vitest'
import { ChannelReader, ChannelWriter, registerWorker, TriggerAction } from '../src/index'

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
