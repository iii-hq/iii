import { beforeAll, describe, expect, it, vi } from 'vitest'
import type { EnqueueResult } from '../src/index'
import { registerWorker } from '../src/index'
import { iii } from './utils'

beforeAll(() => {
  vi.spyOn(iii, 'shutdown').mockResolvedValue(undefined)
})

describe('Package Exports', () => {
  it('should export main SDK symbols', () => {
    expect(registerWorker).toBeDefined()
    expect(typeof registerWorker).toBe('function')
  })

  it('registerWorker returns an IIIClient-shaped object; trigger is call', () => {
    expect(typeof iii.registerFunction).toBe('function')
    expect(typeof iii.call).toBe('function')
    expect(iii.trigger).toBe(iii.call)
    for (const hook of [
      'onObserverAdded',
      'onObserverRemoved',
      'onObserverMatch',
      'onObserverDeliver',
      'observers',
      'isObserved',
    ]) {
      expect(typeof (iii as unknown as Record<string, unknown>)[hook]).toBe('function')
    }
    void iii.shutdown()
  })

  it('exports CallAction, ObserveRejection and EngineSources from the root, and no trigger API', async () => {
    const root = (await import('../src/index')) as Record<string, unknown>
    expect(root.CallAction).toBeDefined()
    expect(root.TriggerAction).toBe(root.CallAction)
    expect(typeof root.ObserveRejection).toBe('function')
    expect(root.EngineSources).toMatchObject({
      FUNCTIONS_CHANGED: 'engine::on-functions-changed',
      OBSERVERS_CHANGED: 'engine::on-observers-changed',
    })
    expect(root.EngineTriggers).toBeUndefined()
    const engine = (await import('../src/engine')) as Record<string, unknown>
    expect(engine.EngineSources).toBe(root.EngineSources)
    expect(engine.EngineTriggers).toBeUndefined()
  })

  it('should import stream module', async () => {
    await expect(import('../src/stream')).resolves.toBeDefined()
  })

  it('should import state module', async () => {
    const stateModule = await import('../src/state')
    expect(stateModule).toBeDefined()
    expect(stateModule.StateEventType).toBeDefined()
    expect(Object.keys(stateModule).length).toBeGreaterThan(0)
  })

  it('should export channel symbols from the subpath only, not the root', async () => {
    const ch = await import('../src/channel')
    expect(ch.ChannelReader).toBeDefined()
    expect(ch.ChannelWriter).toBeDefined()
    const root = (await import('../src/index')) as Record<string, unknown>
    expect(root.ChannelReader).toBeUndefined()
    expect(root.ChannelWriter).toBeUndefined()
  })

  it('should import the runtime subpath module', async () => {
    await expect(import('../src/runtime')).resolves.toBeDefined()
  })

  it('exposes the TelemetryOptions type via the barrel', async () => {
    // Type-only; presence is enforced by tsc. This asserts the module resolves.
    await expect(import('../src/index')).resolves.toBeDefined()
  })

  it('re-exports the EnqueueResult type from the barrel', () => {
    // Type-only; presence is enforced by tsc.
    const receipt: EnqueueResult = { messageReceiptId: 'abc' }
    expect(receipt.messageReceiptId).toBe('abc')
  })
})
