import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetchStateItems } from './state'

function mockStateItems(payload: unknown): void {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({
      ok: true,
      json: async () => payload,
    })),
  )
}

describe('fetchStateItems', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('uses explicit item keys', async () => {
    mockStateItems({ items: [{ key: 'worker', value: { status: 'ready' } }] })

    const { items } = await fetchStateItems('runtime')

    expect(items[0]).toMatchObject({
      groupId: 'runtime',
      key: 'worker',
      value: { status: 'ready' },
      type: 'object',
    })
  })

  it('uses map keys for object-shaped item responses', async () => {
    mockStateItems({ items: { 'files/worker/config.yaml': { raw: true } } })

    const { items } = await fetchStateItems('runtime')

    expect(items[0].key).toBe('files/worker/config.yaml')
    expect(items[0].value).toEqual({ raw: true })
  })

  it('uses explicit item keys before generated object entry keys', async () => {
    mockStateItems({ items: { 'item-0': { key: 'worker', value: { status: 'ready' } } } })

    const { items } = await fetchStateItems('runtime')

    expect(items[0].key).toBe('worker')
    expect(items[0].value).toEqual({ status: 'ready' })
  })

  it('uses embedded state_key before falling back', async () => {
    mockStateItems({ items: [{ state_key: 'last_test_suite', status: 'PASS' }] })

    const { items } = await fetchStateItems('runtime')

    expect(items[0].key).toBe('last_test_suite')
    expect(items[0].value).toEqual({ state_key: 'last_test_suite', status: 'PASS' })
  })

  it('does not invent item-N labels when keys are missing', async () => {
    mockStateItems({ items: [{ status: 'unknown' }] })

    const { items } = await fetchStateItems('runtime')

    expect(items[0].key).toBe('(missing key 0)')
  })
})
