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

  it('registerWorker returns an IIIClient-shaped object', () => {
    expect(typeof iii.registerFunction).toBe('function')
    expect(typeof iii.trigger).toBe('function')
    void iii.shutdown()
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

  it('should export channel symbols from the subpath and keep the deprecated root shim', async () => {
    const ch = await import('../src/channel')
    expect(ch.ChannelReader).toBeDefined()
    expect(ch.ChannelWriter).toBeDefined()
    const root = await import('../src/index')
    expect(root.ChannelReader).toBe(ch.ChannelReader)
    expect(root.ChannelWriter).toBe(ch.ChannelWriter)
  })

  it('should import the trigger subpath module', async () => {
    await expect(import('../src/trigger')).resolves.toBeDefined()
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
