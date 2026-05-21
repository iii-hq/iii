import { describe, expect, it } from 'vitest'
import { currentSpanId, currentTraceId } from './context'

describe('context', () => {
  it('returns undefined outside any span', () => {
    expect(currentSpanId()).toBeUndefined()
    expect(currentTraceId()).toBeUndefined()
  })
})
