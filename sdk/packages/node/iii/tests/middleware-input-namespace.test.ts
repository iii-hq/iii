import { describe, expect, it } from 'vitest'
import type { MiddlewareFunctionInput } from '../src/index'

// A typed middleware must be able to read the target namespace the engine sends
// so it can re-target the forwarded call at the caller's namespace.
// Compiled by `tsc --noEmit` (CI), so a missing `namespace` field fails to type-check.
describe('MiddlewareFunctionInput namespace', () => {
  it('exposes the namespace the engine addressed the invoke to', () => {
    // Mirrors the wire object the engine builds (engine/src/engine/mod.rs).
    const wire = {
      function_id: 'orders::create',
      payload: {},
      context: {},
      namespace: 'orders',
    }
    const input = wire as MiddlewareFunctionInput
    expect(input.namespace).toBe('orders')
  })

  it('treats namespace as optional', () => {
    const input: MiddlewareFunctionInput = {
      function_id: 'orders::create',
      payload: {},
      context: {},
    }
    expect(input.namespace).toBeUndefined()
  })
})
