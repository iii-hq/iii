import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetchWorkers } from './workers'

function mockFetchResolving(payload: unknown): void {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({
      ok: true,
      json: async () => payload,
    })),
  )
}

describe('fetchWorkers', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  // Regression for iii-hq/iii#1708: in-process/built-in workers (e.g. `configuration`)
  // are reported with `function_count` but no `functions` array. The worker detail page
  // reads `selectedWorker.functions.length`, which threw "Cannot read properties of
  // undefined (reading 'length')" before normalization.
  it('defaults `functions` to an empty array when the engine omits it', async () => {
    mockFetchResolving({
      workers: [{ id: 'configuration', name: 'configuration', function_count: 5 }],
      timestamp: 1,
    })

    const { workers } = await fetchWorkers()

    expect(workers[0].functions).toEqual([])
    expect(() => workers[0].functions.length).not.toThrow()
  })

  it('preserves an existing `functions` array for external SDK workers', async () => {
    mockFetchResolving({
      workers: [{ id: 'w1', name: 'sdk', function_count: 2, functions: ['a::run', 'b::run'] }],
      timestamp: 1,
    })

    const { workers } = await fetchWorkers()

    expect(workers[0].functions).toEqual(['a::run', 'b::run'])
  })
})
