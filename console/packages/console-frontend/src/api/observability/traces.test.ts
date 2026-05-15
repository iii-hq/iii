import { describe, expect, it, vi } from 'vitest'
import type { ISdk, TriggerRequest } from 'iii-browser-sdk'
import { clearTraces, fetchTraces, fetchTracesGroupBy, fetchTraceTree } from './traces'

function makeSdkStub(
  responder: (req: TriggerRequest<unknown>) => unknown | Promise<unknown>,
): { sdk: ISdk; trigger: ReturnType<typeof vi.fn> } {
  const trigger = vi.fn(async (req: TriggerRequest<unknown>) => responder(req))
  // Only `trigger` is exercised; the rest of ISdk is unused in these tests.
  const sdk = { trigger } as unknown as ISdk
  return { sdk, trigger }
}

describe('fetchTraces', () => {
  it('invokes engine::traces::list with defaulted offset and limit', async () => {
    const expected = { spans: [], total: 0, offset: 0, limit: 100 }
    const { sdk, trigger } = makeSdkStub(() => expected)

    const result = await fetchTraces(sdk)

    expect(trigger).toHaveBeenCalledTimes(1)
    const call = trigger.mock.calls[0][0]
    expect(call.function_id).toBe('engine::traces::list')
    expect(call.payload).toMatchObject({ offset: 0, limit: 100 })
    expect(call.timeoutMs).toBe(5000)
    expect(result).toEqual(expected)
  })

  it('forwards caller-supplied filters (include_internal, search_all_spans, name)', async () => {
    const { sdk, trigger } = makeSdkStub(() => ({ spans: [], total: 0, offset: 0, limit: 50 }))

    await fetchTraces(sdk, {
      include_internal: true,
      search_all_spans: true,
      name: 'foo',
      limit: 50,
    })

    const payload = trigger.mock.calls[0][0].payload as Record<string, unknown>
    expect(payload.include_internal).toBe(true)
    expect(payload.search_all_spans).toBe(true)
    expect(payload.name).toBe('foo')
    expect(payload.limit).toBe(50)
    expect(payload.offset).toBe(0)
  })

  it('returns an empty TracesResponse when the engine reports memory exporter not enabled', async () => {
    const { sdk } = makeSdkStub(() => {
      throw new Error('Memory exporter is not enabled')
    })

    const result = await fetchTraces(sdk, { limit: 25 })

    expect(result).toEqual({ spans: [], total: 0, offset: 0, limit: 25 })
  })

  it('rethrows any other error as an Error instance', async () => {
    const { sdk } = makeSdkStub(() => {
      throw new Error('something else')
    })

    await expect(fetchTraces(sdk)).rejects.toThrow('something else')
  })
})

describe('fetchTraceTree', () => {
  it('invokes engine::traces::tree with the given trace_id', async () => {
    const expected = { roots: [] }
    const { sdk, trigger } = makeSdkStub(() => expected)

    const result = await fetchTraceTree(sdk, 'abc123')

    expect(trigger).toHaveBeenCalledTimes(1)
    const call = trigger.mock.calls[0][0]
    expect(call.function_id).toBe('engine::traces::tree')
    expect(call.payload).toEqual({ trace_id: 'abc123' })
    expect(call.timeoutMs).toBe(10000)
    expect(result).toEqual(expected)
  })

  it('returns { roots: [] } when the engine reports memory exporter not enabled', async () => {
    const { sdk } = makeSdkStub(() => {
      throw new Error('memory exporter not enabled')
    })

    const result = await fetchTraceTree(sdk, 'abc123')

    expect(result).toEqual({ roots: [] })
  })

  it('rethrows any other error as an Error instance', async () => {
    const { sdk } = makeSdkStub(() => {
      throw new Error('something else')
    })

    await expect(fetchTraceTree(sdk, 'abc123')).rejects.toThrow('something else')
  })
})

describe('clearTraces', () => {
  it('invokes engine::traces::clear with empty payload and returns success', async () => {
    const { sdk, trigger } = makeSdkStub(() => ({ success: true }))

    const result = await clearTraces(sdk)

    expect(trigger).toHaveBeenCalledTimes(1)
    const call = trigger.mock.calls[0][0]
    expect(call.function_id).toBe('engine::traces::clear')
    expect(call.payload).toEqual({})
    expect(call.timeoutMs).toBe(5000)
    expect(result).toEqual({ success: true })
  })

  it('rethrows engine errors as Error', async () => {
    const { sdk } = makeSdkStub(() => {
      throw new Error('boom')
    })

    await expect(clearTraces(sdk)).rejects.toThrow('boom')
  })

  it('does not swallow memory-exporter-not-enabled errors (unlike fetch functions)', async () => {
    const { sdk } = makeSdkStub(() => {
      throw new Error('Memory exporter is not enabled')
    })

    await expect(clearTraces(sdk)).rejects.toThrow('Memory exporter is not enabled')
  })
})

describe('fetchTracesGroupBy', () => {
  it('invokes engine::traces::group_by with the given attribute', async () => {
    const expected = {
      groups: [
        {
          value: 'msg-1',
          trace_ids: ['t1', 't2'],
          span_count: 5,
          first_seen_ms: 1000,
          last_seen_ms: 2000,
          duration_ms: 1000,
          error_count: 0,
        },
      ],
    }
    const { sdk, trigger } = makeSdkStub(() => expected)

    const result = await fetchTracesGroupBy(sdk, { attribute: 'iii.message.id' })

    expect(trigger).toHaveBeenCalledTimes(1)
    const call = trigger.mock.calls[0][0]
    expect(call.function_id).toBe('engine::traces::group_by')
    expect(call.payload).toEqual({ attribute: 'iii.message.id' })
    expect(call.timeoutMs).toBe(5000)
    expect(result).toEqual(expected)
  })

  it('forwards optional filters (since_ms, limit, include_internal)', async () => {
    const { sdk, trigger } = makeSdkStub(() => ({ groups: [] }))

    await fetchTracesGroupBy(sdk, {
      attribute: 'iii.session.id',
      since_ms: 1_700_000_000_000,
      limit: 25,
      include_internal: true,
    })

    expect(trigger.mock.calls[0][0].payload).toEqual({
      attribute: 'iii.session.id',
      since_ms: 1_700_000_000_000,
      limit: 25,
      include_internal: true,
    })
  })

  it('strips undefined optional fields from the payload', async () => {
    const { sdk, trigger } = makeSdkStub(() => ({ groups: [] }))

    await fetchTracesGroupBy(sdk, {
      attribute: 'iii.function.id',
      since_ms: undefined,
      limit: undefined,
    })

    expect(trigger.mock.calls[0][0].payload).toEqual({ attribute: 'iii.function.id' })
  })

  it('returns { groups: [] } when the engine reports memory exporter not enabled', async () => {
    const { sdk } = makeSdkStub(() => {
      throw new Error('memory exporter not enabled')
    })

    const result = await fetchTracesGroupBy(sdk, { attribute: 'iii.message.id' })

    expect(result).toEqual({ groups: [] })
  })

  it('rethrows any other error (e.g. function_not_found from older engines) as Error', async () => {
    const { sdk } = makeSdkStub(() => {
      throw new Error('function_not_found: engine::traces::group_by')
    })

    await expect(
      fetchTracesGroupBy(sdk, { attribute: 'iii.message.id' }),
    ).rejects.toThrow('function_not_found: engine::traces::group_by')
  })
})
