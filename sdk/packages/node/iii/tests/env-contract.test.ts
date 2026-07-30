import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { DEFAULT_ENGINE_URL, registerWorker } from '../src/iii'

/**
 * `registerWorker()` with no address: the supervisor that spawned this process
 * (`iii compose`, a container runtime, systemd) sets III_URL, the same way it
 * sets III_NAMESPACE and III_WORKER_NAME.
 */
describe('registerWorker — engine address resolution', () => {
  let previous: string | undefined

  beforeEach(() => {
    previous = process.env.III_URL
    delete process.env.III_URL
  })

  afterEach(() => {
    if (previous === undefined) {
      delete process.env.III_URL
    } else {
      process.env.III_URL = previous
    }
  })

  it('falls back to the IPv4 loopback default when nothing is set', () => {
    const worker = registerWorker()
    expect(worker.getAddress()).toBe(DEFAULT_ENGINE_URL)
    expect(DEFAULT_ENGINE_URL).toBe('ws://127.0.0.1:49134')
  })

  it('reads III_URL when no address is passed', () => {
    process.env.III_URL = 'ws://engine.example:9000'
    expect(registerWorker().getAddress()).toBe('ws://engine.example:9000')
  })

  it('an explicit address wins over III_URL', () => {
    process.env.III_URL = 'ws://from-env:1'
    expect(registerWorker('ws://explicit:2').getAddress()).toBe('ws://explicit:2')
  })

  it('ignores an empty III_URL', () => {
    process.env.III_URL = ''
    expect(registerWorker().getAddress()).toBe(DEFAULT_ENGINE_URL)
  })

  it('still accepts options when the address is omitted', () => {
    process.env.III_URL = 'ws://engine.example:9000'
    const worker = registerWorker(undefined, { workerName: 'my-worker' })
    expect(worker.getAddress()).toBe('ws://engine.example:9000')
  })
})
