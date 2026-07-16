import { afterEach, describe, expect, it, vi } from 'vitest'

import { randomUUID } from '../src/utils'

const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

describe('randomUUID', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('returns a v4 UUID when crypto.randomUUID is available', () => {
    expect(randomUUID()).toMatch(UUID_V4)
  })

  it('returns a v4 UUID in insecure contexts where crypto.randomUUID is undefined', () => {
    // Browsers only expose crypto.randomUUID in secure contexts (https /
    // localhost); getRandomValues is always available.
    vi.stubGlobal('crypto', {
      getRandomValues: crypto.getRandomValues.bind(crypto),
    })
    expect(typeof crypto.randomUUID).toBe('undefined')
    expect(randomUUID()).toMatch(UUID_V4)
  })

  it('generates unique ids', () => {
    vi.stubGlobal('crypto', {
      getRandomValues: crypto.getRandomValues.bind(crypto),
    })
    const ids = new Set(Array.from({ length: 1000 }, () => randomUUID()))
    expect(ids.size).toBe(1000)
  })
})
