import { describe, expect, it, vi } from 'vitest'
import { executeTracedRequest } from './http-instrumentation'

describe('executeTracedRequest', () => {
  it('forwards to fetch and returns the response', async () => {
    const originalFetch = globalThis.fetch
    globalThis.fetch = vi.fn().mockResolvedValue(new Response('ok', { status: 200 }))
    try {
      const res = await executeTracedRequest('https://example.com/api')
      expect(res.status).toBe(200)
      expect(await res.text()).toBe('ok')
    } finally {
      globalThis.fetch = originalFetch
    }
  })
})
