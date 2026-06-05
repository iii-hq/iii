import { beforeAll, describe, expect, it, vi } from 'vitest'
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

  it('should import stream module', async () => {
    await expect(import('../src/stream')).resolves.toBeDefined()
  })

  it('should import state module', async () => {
    const stateModule = await import('../src/state')
    expect(stateModule).toBeDefined()
    expect(stateModule.StateEventType).toBeDefined()
    expect(Object.keys(stateModule).length).toBeGreaterThan(0)
  })
})
