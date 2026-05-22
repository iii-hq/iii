import { beforeAll, describe, expect, it, vi } from 'vitest'
import { registerWorker, Logger } from '../src/index'
import { initOtel, shutdownOtel, getLogger } from '../src/telemetry'
import { iii } from './utils'

beforeAll(() => {
  vi.spyOn(iii, 'shutdown').mockResolvedValue(undefined)
})

describe('Package Exports', () => {
  it('should export main SDK symbols', () => {
    expect(registerWorker).toBeDefined()
    expect(typeof registerWorker).toBe('function')
    expect(Logger).toBeDefined()
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

  it('should export supported telemetry utilities', () => {
    expect(initOtel).toBeDefined()
    expect(typeof initOtel).toBe('function')
    expect(shutdownOtel).toBeDefined()
    expect(typeof shutdownOtel).toBe('function')
    expect(getLogger).toBeDefined()
    expect(typeof getLogger).toBe('function')
  })

  it('should expose tracer/meter accessors and span utilities on public telemetry surface', async () => {
    const telemetryExports = await import('../src/telemetry')
    const keys = Object.keys(telemetryExports)
    expect(keys).toContain('getTracer')
    expect(keys).toContain('getMeter')
    expect(keys).toContain('SpanKind')
  })
})
