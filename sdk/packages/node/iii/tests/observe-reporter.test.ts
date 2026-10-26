import { afterEach, describe, expect, it, vi } from 'vitest'
import { Reporter, type ReporterHost, type ReportInit, wellFormed } from '../src/reporter'

/** The reporter (sdk.md §4.4, CONTRACT D12): batching, bounds, drops, splitting, offline, stats. */

type Frame = { type: string; items?: Record<string, unknown>[] } & Record<string, unknown>

function host(initial: { online?: boolean; status?: 'ready' | 'wait' | 'gone' } = {}) {
  const state = { online: initial.online ?? true, status: initial.status ?? 'ready', pending: 0 }
  const frames: string[] = []
  const h: ReporterHost & { state: typeof state; frames: string[]; parsed(): Frame[] } = {
    state,
    frames,
    online: () => state.online,
    status: () => state.status as 'ready' | 'wait' | 'gone',
    sendFrame: (data) => {
      if (!state.online) return false
      frames.push(data)
      return true
    },
    pendingBytes: () => state.pending,
    logError: () => undefined,
    parsed: () => frames.map((f) => JSON.parse(f) as Frame),
  }
  return h
}

function item(n: number, extra: Partial<ReportInit> = {}): ReportInit {
  return {
    function_id: 'fn::a',
    namespace: 'default',
    invocation_id: `inv-${n}`,
    matched: ['o-1'],
    status: 'success',
    values: { input: { n } },
    captures: ['input'],
    started_at_ms: 1,
    duration_ms: 0.5,
    observe_depth: 0,
    ...extra,
  }
}

const reporters: Reporter[] = []
function reporter(h: ReporterHost, limits?: ConstructorParameters<typeof Reporter>[1]) {
  const r = new Reporter(h, limits)
  reporters.push(r)
  return r
}

afterEach(() => {
  for (const r of reporters.splice(0)) r.stop()
  vi.useRealTimers()
})

describe('Reporter', () => {
  it('flushes at 256 items, in one frame, without waiting', () => {
    const h = host()
    const r = reporter(h)
    for (let i = 0; i < 255; i++) r.push(item(i))
    expect(h.frames).toHaveLength(0)
    r.push(item(255))
    expect(h.parsed()).toHaveLength(1)
    expect(h.parsed()[0].type).toBe('invocationsobserved')
    expect(h.parsed()[0].items).toHaveLength(256)
    expect(r.size).toBe(0)
  })

  it('flushes a smaller batch after the 5 ms linger', async () => {
    vi.useFakeTimers()
    const h = host()
    const r = reporter(h)
    r.push(item(1))
    r.push(item(2))
    await vi.advanceTimersByTimeAsync(4)
    expect(h.frames).toHaveLength(0)
    await vi.advanceTimersByTimeAsync(1)
    expect(h.parsed().map((f) => f.items?.map((i) => i.invocation_id))).toEqual([['inv-1', 'inv-2']])
  })

  it('is bounded by items: drops the newest and counts BUFFER_FULL', () => {
    const h = host({ online: false })
    const r = reporter(h, { maxItems: 3 })
    const accepted = [1, 2, 3, 4, 5].map((n) => r.push(item(n)))
    expect(accepted).toEqual([true, true, true, false, false])
    h.state.online = true
    r.flush()
    expect(h.parsed()[0].items?.map((i) => i.invocation_id)).toEqual(['inv-1', 'inv-2', 'inv-3'])
    r.sendStats(Date.now())
    expect(h.parsed()[1]).toMatchObject({
      type: 'localinvocationstats',
      functions: [],
      dropped_reports: [{ function_id: 'fn::a', reason: 'BUFFER_FULL', count: 2 }],
    })
  })

  it('is bounded by bytes, and an item larger than the buffer is TOO_LARGE', () => {
    const h = host({ online: false })
    const r = reporter(h, { maxBytes: 1000 })
    expect(r.push(item(1, { values: { input: 'x'.repeat(2000) } }))).toBe(false)
    expect(r.push(item(2, { values: { input: 'x'.repeat(600) } }))).toBe(true)
    expect(r.push(item(3, { values: { input: 'x'.repeat(600) } }))).toBe(false)
    h.state.online = true
    r.sendStats()
    expect(h.parsed()[0].dropped_reports).toEqual([
      { function_id: 'fn::a', reason: 'TOO_LARGE', count: 1 },
      { function_id: 'fn::a', reason: 'BUFFER_FULL', count: 1 },
    ])
  })

  it('splits a flush to stay under the frame limit', () => {
    const h = host({ online: false })
    const r = reporter(h, { maxFrameBytes: 700, maxBytes: 100_000 })
    for (let i = 0; i < 6; i++) r.push(item(i, { values: { input: 'y'.repeat(200) } }))
    h.state.online = true
    r.flush()
    expect(h.frames.length).toBeGreaterThan(1)
    for (const f of h.frames) expect(Buffer.byteLength(f)).toBeLessThanOrEqual(700)
    expect(h.parsed().flatMap((f) => f.items?.map((i) => i.invocation_id))).toEqual([
      'inv-0',
      'inv-1',
      'inv-2',
      'inv-3',
      'inv-4',
      'inv-5',
    ])
  })

  it('keeps items while offline or until the function is confirmed, then flushes them; drops them when it is gone', () => {
    const h = host({ online: false })
    const r = reporter(h)
    r.push(item(1))
    r.flush()
    expect(h.frames).toHaveLength(0)
    h.state.online = true
    h.state.status = 'wait'
    r.flush()
    expect(h.frames).toHaveLength(0)
    expect(r.size).toBe(1)
    h.state.status = 'ready'
    r.flush()
    expect(h.parsed()[0].items).toHaveLength(1)

    h.state.status = 'gone'
    r.push(item(2))
    r.flush()
    expect(h.frames).toHaveLength(1)
    expect(r.size).toBe(0)
    r.sendStats()
    expect(h.parsed()[1].dropped_reports).toEqual([{ function_id: 'fn::a', reason: 'INACTIVE', count: 1 }])
  })

  it('puts a batch back when the socket closes under the flush, within the bounds', () => {
    const h = host({ online: false })
    const r = reporter(h, { maxItems: 3 })
    for (const n of [1, 2]) r.push(item(n))
    h.state.online = true
    h.sendFrame = () => false // closed between the check and the send
    r.flush()
    expect(r.size).toBe(2)
    r.push(item(3))
    r.push(item(4)) // the buffer is full again: BUFFER_FULL
    h.sendFrame = (data) => {
      h.frames.push(data)
      return true
    }
    r.flush()
    expect(h.parsed()[0].items?.map((i) => i.invocation_id)).toEqual(['inv-1', 'inv-2', 'inv-3'])
  })

  it('the bound counts the bytes the socket has not written yet (a peer that stops reading)', async () => {
    vi.useFakeTimers()
    const h = host()
    const r = reporter(h, { maxBytes: 2000 })
    h.state.pending = 1900 // handed to ws, not written
    expect(r.push(item(1, { values: { input: 'x'.repeat(200) } }))).toBe(false) // BUFFER_FULL
    h.state.pending = 1500
    expect(r.push(item(2, { values: { input: 'x'.repeat(200) } }))).toBe(true)
    h.state.pending = 1800
    r.flush() // 1800 pending + this frame would pass 2000: kept, not handed over
    expect(h.frames).toHaveLength(0)
    expect(r.size).toBe(1)
    h.state.pending = 0 // the peer reads again
    await vi.advanceTimersByTimeAsync(100) // the retry
    expect(h.parsed()[0].items?.map((i) => i.invocation_id)).toEqual(['inv-2'])
    r.sendStats()
    expect(h.parsed()[1].dropped_reports).toEqual([{ function_id: 'fn::a', reason: 'BUFFER_FULL', count: 1 }])
  })

  it('an emit merged into a buffered item stays within the bounds', () => {
    const h = host({ online: false })
    const r = reporter(h, { maxBytes: 1500 })
    const event = { tick: 1, blob: 'b'.repeat(400) }
    const ctx = {}
    const emit = (observer_id: string, captures: string[]) =>
      r.pushEmit({
        function_id: 'cron::on-tick',
        invocation_id: 'inv-e',
        status: 'success',
        values: { input: event, caller: { worker_name: 'c'.repeat(1200) } },
        captures,
        started_at_ms: 1,
        duration_ms: 0,
        observe_depth: 0,
        observer_id,
        event,
        context: ctx,
      })
    expect(emit('a', ['input'])).toBe(true)
    expect(emit('b', ['input', 'caller'])).toBe(false) // the caller part would pass the buffer: TOO_LARGE
    expect(emit('c', ['input'])).toBe(true) // merged: no new part
    h.state.online = true
    r.flush()
    expect(h.parsed()[0].items?.map((i) => [i.matched, 'caller' in i])).toEqual([[['a', 'c'], false]])
    r.sendStats()
    expect(h.parsed()[1].dropped_reports).toEqual([{ function_id: 'cron::on-tick', reason: 'TOO_LARGE', count: 1 }])
  })

  it('never merges an emit into an item a flush already sent (the 256th-item flush happens inside the push)', () => {
    const h = host()
    const r = reporter(h, { batchItems: 2 })
    const ctx = {}
    const e = { tick: 'E' }
    const f = { tick: 'F' }
    const emit = (observer_id: string, event: unknown) =>
      r.pushEmit({
        function_id: 'cron::on-tick',
        invocation_id: `inv-${observer_id}`,
        status: 'success',
        values: { input: event },
        captures: ['input'],
        started_at_ms: 1,
        duration_ms: 0,
        observe_depth: 0,
        observer_id,
        event,
        context: ctx,
      })
    emit('A', f)
    emit('A', e) // the second item: flushed now, inside the push
    expect(h.frames).toHaveLength(1)
    emit('B', e) // same event, same context: a new item, not a merge into the sent one
    r.flush()
    expect(h.parsed().flatMap((fr) => fr.items?.map((i) => [i.input, i.matched]))).toEqual([
      [{ tick: 'F' }, ['A']],
      [{ tick: 'E' }, ['A']],
      [{ tick: 'E' }, ['B']],
    ])
  })

  it('an emit merged into an item put back after a failed send is sent exactly once', () => {
    const h = host()
    const r = reporter(h)
    const event = { tick: 1 }
    const ctx = {}
    const emit = (observer_id: string) =>
      r.pushEmit({
        function_id: 'cron::on-tick',
        invocation_id: 'inv-1',
        status: 'success',
        values: { input: event },
        captures: ['input'],
        started_at_ms: 1,
        duration_ms: 0,
        observe_depth: 0,
        observer_id,
        event,
        context: ctx,
      })
    emit('A')
    const send = h.sendFrame
    h.sendFrame = () => false // the socket closed under the flush
    r.flush()
    expect(r.size).toBe(1)
    h.sendFrame = send
    emit('B') // merges into the requeued item
    expect(r.size).toBe(1)
    r.flush()
    r.flush()
    expect(
      h
        .parsed()
        .flatMap((f) => f.items ?? [])
        .map((i) => i.matched),
    ).toEqual([['A', 'B']])
  })

  it('an undefined event emitted outside any handler coalesces also after the buffer was re-indexed', () => {
    const h = host({ online: false })
    const r = reporter(h)
    const emit = (observer_id: string) =>
      r.pushEmit({
        function_id: 'cron::on-tick',
        invocation_id: `inv-${observer_id}`,
        status: 'success',
        values: { input: undefined },
        captures: ['input'],
        started_at_ms: 1,
        duration_ms: 0,
        observe_depth: 0,
        observer_id,
        event: undefined,
        context: undefined,
      })
    emit('A')
    r.push(item(1, { function_id: 'fn::other' }))
    r.discard('fn::other') // rebuilds the buffer and its index
    emit('B')
    h.state.online = true
    r.flush()
    expect(h.parsed()[0].items?.map((i) => i.matched)).toEqual([['A', 'B']])
  })

  it("discards a function's held items as INACTIVE, keeping the others", () => {
    const h = host({ online: false })
    const r = reporter(h)
    r.push(item(1))
    r.push(item(2, { function_id: 'fn::b' }))
    r.push(item(3))
    r.discard('fn::a')
    expect(r.size).toBe(1)
    h.state.online = true
    r.flush()
    expect(h.parsed()[0].items?.map((i) => i.invocation_id)).toEqual(['inv-2'])
    r.sendStats()
    expect(h.parsed()[1].dropped_reports).toEqual([{ function_id: 'fn::a', reason: 'INACTIVE', count: 2 }])
  })

  it('a flush while every function waits looks at each function once, not at every item', () => {
    const h = host({ status: 'wait' })
    const status = vi.fn(() => 'wait' as const)
    h.status = status
    const r = reporter(h, { batchItems: 1_000_000 })
    for (let i = 0; i < 5000; i++) r.push(item(i))
    status.mockClear()
    for (let i = 0; i < 100; i++) r.flush()
    expect(status).toHaveBeenCalledTimes(100)
    expect(h.frames).toHaveLength(0)
  })

  it('sends localinvocationstats only when non-zero, and keeps counting while offline', () => {
    const h = host({ online: false })
    const r = reporter(h)
    expect(r.sendStats(1000)).toBe(false)
    r.recordLocal('fn::a', true, 1.5)
    r.recordLocal('fn::a', false, 2)
    expect(r.sendStats(2000)).toBe(false) // offline: kept
    h.state.online = true
    r.recordLocal('fn::b', true, 0.25)
    expect(r.sendStats(11_000)).toBe(true)
    expect(h.parsed()).toEqual([
      {
        type: 'localinvocationstats',
        window_ms: 10_000,
        functions: [
          { function_id: 'fn::a', count: 2, errors: 1, duration_ms_sum: 3.5 },
          { function_id: 'fn::b', count: 1, errors: 0, duration_ms_sum: 0.25 },
        ],
        dropped_reports: [],
      },
    ])
    expect(r.sendStats(21_000)).toBe(false)
    expect(h.frames).toHaveLength(1)
  })

  it('sends stats on its 10 s timer', async () => {
    vi.useFakeTimers()
    const h = host()
    const r = reporter(h)
    r.start()
    r.recordLocal('fn::a', true, 1)
    await vi.advanceTimersByTimeAsync(9_999)
    expect(h.frames).toHaveLength(0)
    await vi.advanceTimersByTimeAsync(1)
    expect(h.parsed()[0]).toMatchObject({ type: 'localinvocationstats', window_ms: 10_000 })
    await vi.advanceTimersByTimeAsync(10_000)
    expect(h.frames).toHaveLength(1)
  })

  it('coalesces one event emitted to N observers from one context into one item, with the union of captures', () => {
    const h = host({ online: false })
    const r = reporter(h)
    const event = { tick: 1 }
    const ctx = {}
    const emit = (observer_id: string, captures: string[], context: unknown = ctx, e: unknown = event) =>
      r.pushEmit({
        function_id: 'cron::on-tick',
        invocation_id: `inv-${observer_id}`,
        status: 'success',
        values: { input: e, caller: { worker_name: 'cron' } },
        captures,
        started_at_ms: 1,
        duration_ms: 0,
        observe_depth: 0,
        observer_id,
        event: e,
        context,
      })
    emit('a', ['input'])
    emit('b', ['caller'])
    emit('a', ['input']) // the same observer again: a second event
    emit('c', ['input'], {}) // another invocation context
    emit('d', ['input'], ctx, { tick: 1 }) // an equal but different event value
    h.state.online = true
    r.flush()
    const items = h.parsed()[0].items ?? []
    expect(items.map((i) => i.matched)).toEqual([['a', 'b'], ['a'], ['c'], ['d']])
    expect(items[0]).toMatchObject({ input: { tick: 1 }, caller: { worker_name: 'cron' } })
    expect(items[1]).not.toHaveProperty('caller')
  })

  it('replaces lone surrogates with U+FFFD in strings and keys, so the engine can decode the frame', () => {
    const h = host()
    const r = reporter(h)
    r.push(
      item(1, {
        values: {
          input: { s: '\ud800x', '\udc00k': 'v', pair: '😀', text: 'a\\ud800b' },
          caller: { worker_name: 'w\udbff' },
          metadata: { m: ['\udfff'] },
        },
        captures: ['input', 'caller', 'metadata'],
      }),
    )
    r.flush()
    const [text] = h.frames
    // what a strict decoder (serde_json) refuses: an escaped lone surrogate
    expect(text).not.toMatch(/(^|[^\\])(\\\\)*\\ud[89a-f][0-9a-f]{2}/)
    expect(text).toContain('\\ufffd')
    const decoded = JSON.parse(text).items[0]
    expect(decoded.input).toEqual({ s: '\ufffdx', '\ufffdk': 'v', pair: '😀', text: 'a\\ud800b' })
    expect(decoded.caller).toEqual({ worker_name: 'w\ufffd' })
    expect(decoded.metadata).toEqual({ m: ['\ufffd'] })
    // an escaped backslash followed by `ud800` is text: left as is
    expect(wellFormed('{"a":"\\\\ud800"}')).toBe('{"a":"\\\\ud800"}')
  })

  it('never throws on a value JSON cannot encode; writes BigInt as a number', () => {
    const h = host()
    const errors: string[] = []
    h.logError = (m) => errors.push(m)
    const r = reporter(h)
    const cyclic: Record<string, unknown> = {}
    cyclic.self = cyclic
    expect(r.push(item(1, { values: { input: cyclic } }))).toBe(false)
    expect(errors).toHaveLength(1)
    expect(r.push(item(2, { values: { input: { big: 12n } } }))).toBe(true)
    r.flush()
    expect(h.parsed()[0].items?.[0].input).toEqual({ big: 12 })
  })
})
