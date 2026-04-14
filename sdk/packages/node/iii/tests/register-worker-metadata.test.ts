import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../src/iii'

type InternalSdk = {
  trigger: ReturnType<typeof vi.fn>
  registerWorkerMetadata: () => void
}

describe('registerWorkerMetadata — isolation field', () => {
  let previous: string | undefined

  beforeEach(() => {
    previous = process.env.III_ISOLATION
    delete process.env.III_ISOLATION
  })

  afterEach(() => {
    if (previous === undefined) {
      delete process.env.III_ISOLATION
    } else {
      process.env.III_ISOLATION = previous
    }
  })

  it('sets isolation to null when III_ISOLATION is unset', () => {
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    expect(sdk.trigger).toHaveBeenCalledOnce()
    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.isolation).toBeNull()
  })

  it('forwards the III_ISOLATION value into the payload', () => {
    process.env.III_ISOLATION = 'kubernetes'
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    expect(sdk.trigger).toHaveBeenCalledOnce()
    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.isolation).toBe('kubernetes')
  })

  it('maps empty-string III_ISOLATION to null', () => {
    process.env.III_ISOLATION = ''
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    expect(sdk.trigger).toHaveBeenCalledOnce()
    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.isolation).toBeNull()
  })
})
