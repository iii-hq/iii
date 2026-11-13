import { Readable, Writable } from 'node:stream'
import { describe, expect, it } from 'vitest'
import {
  abortInput,
  asStream,
  forwardableRef,
  handOn,
  handOver,
  InputStream,
  inputFromRef,
  isTransferred,
  markTransferred,
  OutputStream,
  outputDone,
  outputFromRef,
  pumpStream,
  StreamScope,
  streamKind,
} from '../src/call-streams'
import { StreamAbortedError } from '../src/errors'
import type { StreamChannelRef } from '../src/iii-types'
import {
  cloneKeepingStreams,
  containsStreams,
  holdsWriteSide,
  isObservedEnvelope,
  mapStreams,
  wireRef,
} from '../src/stream-walk'

// Streams in this process: no engine, no socket.

const ref = (direction: 'read' | 'write'): StreamChannelRef => ({ channel_id: 'c1', access_key: 'k1', direction })
/** A binding nothing listens on: a test that opened its socket would fail. */
const nowhere = { wsBase: 'ws://127.0.0.1:1' }

function collector(): { sink: Writable; chunks: Buffer[]; finished: () => boolean } {
  const chunks: Buffer[] = []
  let done = false
  const sink = new Writable({
    write(chunk, _e, cb) {
      chunks.push(chunk)
      cb()
    },
    final(cb) {
      done = true
      cb()
    },
  })
  return { sink, chunks, finished: () => done }
}

describe('InputStream.from', () => {
  it.each([
    ['a string', () => 'héllo'],
    ['a Buffer', () => Buffer.from('héllo')],
    ['a Uint8Array', () => new TextEncoder().encode('héllo')],
    ['an ArrayBuffer', () => new TextEncoder().encode('héllo').buffer],
    ['a Readable', () => Readable.from([Buffer.from('hé'), Buffer.from('llo')])],
    ['a Blob', () => new Blob(['hé', 'llo'])],
    ['a ReadableStream', () => new Blob(['héllo']).stream()],
    [
      'an async generator',
      () =>
        (async function* () {
          yield 'hé'
          yield new TextEncoder().encode('llo')
        })(),
    ],
    ['an array of chunks', () => ['hé', 'llo']],
  ])('reads %s', async (_name, make) => {
    expect(await InputStream.from(make() as never).text()).toBe('héllo')
  })

  it('returns an InputStream as it is', () => {
    const input = InputStream.from('x')
    expect(InputStream.from(input)).toBe(input)
  })

  it('refuses what is not a source', () => {
    expect(() => InputStream.from(42 as never)).toThrow(TypeError)
  })

  it('reads nothing until the stream is read', async () => {
    let pulled = 0
    const input = InputStream.from(
      (async function* () {
        pulled++
        yield 'x'
      })(),
    )
    await new Promise((r) => setTimeout(r, 10))
    expect(pulled).toBe(0)
    await input.bytes()
    expect(pulled).toBe(1)
  })
})

describe('InputStream reads', () => {
  it('lines() splits on \\n and \\r\\n, across chunks, and yields the last line without a newline', async () => {
    const lines: string[] = []
    for await (const line of InputStream.from(['a,1\r\nb,', '2\nc', ',3']).lines()) lines.push(line)
    expect(lines).toEqual(['a,1', 'b,2', 'c,3'])
  })

  it('lines() decodes a multi-byte character split across chunks', async () => {
    const bytes = Buffer.from('é\n')
    const lines: string[] = []
    for await (const line of InputStream.from([bytes.subarray(0, 1), bytes.subarray(1)]).lines()) lines.push(line)
    expect(lines).toEqual(['é'])
  })

  it('has one reader (R6): a second read throws, tee() gives two', async () => {
    const input = InputStream.from('data')
    await input.text()
    await expect(input.text()).rejects.toThrow(/one reader/)
    expect(() => input[Symbol.asyncIterator]()).toThrow(/one reader/)

    const [a, b] = InputStream.from(['x', 'y', 'z']).tee()
    const [ta, tb] = await Promise.all([a.text(), b.text()])
    expect([ta, tb]).toEqual(['xyz', 'xyz'])
  })

  it('tee(): cancelling one copy lets the other read everything', async () => {
    const [a, b] = InputStream.from(Array.from({ length: 40 }, (_, i) => `${i},`)).tee()
    await a.cancel()
    expect((await b.text()).split(',').length).toBe(41)
  })

  it('breaking out of for-await cancels the source', async () => {
    let closed = false
    const input = InputStream.from(
      (async function* () {
        try {
          for (;;) yield 'x'
        } finally {
          closed = true
        }
      })(),
    )
    for await (const _ of input) break
    expect(closed).toBe(true)
  })

  it('cancel() on a stream never read closes its source, and is idempotent', async () => {
    const source = new Readable({ read: () => undefined })
    const input = InputStream.from(source)
    await input.cancel()
    await input.cancel()
    expect(source.destroyed).toBe(true)
    // A cancelled stream did not end: reading it fails.
    await expect(input.bytes()).rejects.toMatchObject({
      code: 'STREAM_CLOSED',
      message: 'STREAM_CLOSED: the stream was cancelled',
    })
  })

  it('a source that throws surfaces as the read error after the data before it', async () => {
    const chunks: string[] = []
    const input = InputStream.from(
      (async function* () {
        yield 'first'
        throw new Error('disk read failed')
      })(),
    )
    await expect(
      (async () => {
        for await (const chunk of input) chunks.push(chunk.toString())
      })(),
    ).rejects.toThrow('disk read failed')
    expect(chunks).toEqual(['first'])
  })

  it('toReadable() and toWebStream() give the bytes', async () => {
    const readable = InputStream.from(['a', 'b']).toReadable()
    const parts: Buffer[] = []
    for await (const chunk of readable) parts.push(chunk)
    expect(Buffer.concat(parts).toString()).toBe('ab')

    const web = InputStream.from(['c', 'd']).toWebStream()
    expect(await new Response(web).text()).toBe('cd')
  })

  it('serializes as its StreamInfo, never its access key', () => {
    const remote = inputFromRef(ref('read'), nowhere)
    expect(JSON.stringify({ body: remote })).toBe('{"body":{"stream":true,"direction":"read","id":"c1"}}')
    const local = InputStream.from('x').toJSON()
    expect(local).toMatchObject({ stream: true, direction: 'read' })
    expect(local.id).toMatch(/^local:/)
  })
})

describe('OutputStream.to', () => {
  it('writes into a Writable and ends it on close()', async () => {
    const { sink, chunks, finished } = collector()
    const out = OutputStream.to(sink)
    await out.write('a')
    await out.write(Buffer.from('b'))
    await out.send({ dropped: true })
    await out.close()
    expect(Buffer.concat(chunks).toString()).toBe('ab')
    expect(finished()).toBe(true)
    await expect(outputDone(out)).resolves.toBeUndefined()
  })

  it('abort() destroys the Writable with a StreamAbortedError, and is idempotent', async () => {
    const { sink } = collector()
    const out = OutputStream.to(sink)
    await out.write('a')
    const first = out.abort('render failed')
    expect(out.abort('again')).toBe(first)
    expect(out.close()).toBe(first)
    await first
    expect(sink.destroyed).toBe(true)
    expect(sink.errored).toBeInstanceOf(StreamAbortedError)
    await expect(outputDone(out)).rejects.toMatchObject({ code: 'STREAM_ABORTED', reason: 'render failed' })
    await expect(out.write('late')).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
  })

  it('writes into a Web WritableStream', async () => {
    const parts: string[] = []
    const out = OutputStream.to(
      new WritableStream<Uint8Array>({
        write(chunk) {
          parts.push(Buffer.from(chunk).toString())
        },
      }),
    )
    await out.write('x')
    await out.write('y')
    await out.close()
    expect(parts).toEqual(['x', 'y'])
  })

  it('toWritable() bridges a pipeline; destroying it before the end aborts', async () => {
    const { sink, chunks } = collector()
    const out = OutputStream.to(sink)
    const writable = out.toWritable()
    await new Promise<void>((resolve, reject) =>
      Readable.from(['p', 'q']).pipe(writable).on('finish', resolve).on('error', reject),
    )
    expect(Buffer.concat(chunks).toString()).toBe('pq')

    const second = collector()
    const out2 = OutputStream.to(second.sink)
    const w2 = out2.toWritable()
    w2.on('error', () => undefined)
    w2.destroy(new Error('boom'))
    await expect(outputDone(out2)).rejects.toMatchObject({ reason: 'boom' })
  })

  it('refuses a chunk that is not bytes or a string', async () => {
    const out = OutputStream.to(collector().sink)
    await expect(out.write(42 as never)).rejects.toThrow(TypeError)
  })
})

describe('pumpStream', () => {
  it('copies bytes and closes the output at the end', async () => {
    const { sink, chunks, finished } = collector()
    await pumpStream(InputStream.from(['1', '2', '3']), OutputStream.to(sink))
    expect(Buffer.concat(chunks).toString()).toBe('123')
    expect(finished()).toBe(true)
  })

  it('aborts the output when the input fails, and rejects with STREAM_ABORTED', async () => {
    const { sink } = collector()
    const input = InputStream.from(
      (async function* () {
        yield 'x'
        throw new Error('source failed')
      })(),
    )
    await expect(pumpStream(input, OutputStream.to(sink))).rejects.toMatchObject({
      code: 'STREAM_ABORTED',
      reason: 'source failed',
    })
    expect(sink.destroyed).toBe(true)
  })

  it('cancels the input when the output fails', async () => {
    let closed = false
    const input = InputStream.from(
      (async function* () {
        try {
          for (;;) yield 'x'
        } finally {
          closed = true
        }
      })(),
    )
    const sink = new Writable({ write: (_c, _e, cb) => cb(new Error('disk full')) })
    await expect(pumpStream(input, OutputStream.to(sink))).rejects.toThrow('disk full')
    expect(closed).toBe(true)
  })
})

describe('StreamScope (the handler guard)', () => {
  const settled = (value: unknown) => ({ ok: true as const, value })
  const failed = { ok: false as const, error: new Error('handler failed') }

  it('closes a used OutputStream on success and aborts it on failure', async () => {
    for (const [outcome, expected] of [
      [settled({ any: 'value' }), 'closed'],
      [failed, 'aborted'],
    ] as const) {
      const { sink, finished } = collector()
      const out = OutputStream.to(sink)
      const scope = new StreamScope()
      scope.adopt(out)
      await out.write('x')
      scope.settle(outcome)
      const result = await outputDone(out).then(
        () => 'closed',
        () => 'aborted',
      )
      expect(result).toBe(expected)
      if (expected === 'closed') expect(finished()).toBe(true)
    }
  })

  const untouched = async (
    outcome: { ok: true; value: unknown } | { ok: false; error: unknown },
    mode?: { activation?: boolean; local?: boolean },
  ) => {
    const { sink } = collector()
    const out = OutputStream.to(sink)
    const scope = new StreamScope()
    scope.adopt(out)
    scope.settle(outcome, mode)
    await new Promise((r) => setTimeout(r, 10))
    return sink.writableEnded ? 'closed' : sink.destroyed ? 'aborted' : 'untouched'
  }

  it('closes an untouched OutputStream on success of a plain call, whatever the result (v2)', async () => {
    expect(await untouched(settled(undefined))).toBe('closed')
    expect(await untouched(settled(null))).toBe('closed')
    expect(await untouched(settled({ status_code: 200 }))).toBe('closed')
    expect(await untouched(settled('value'), { local: true })).toBe('closed')
    expect(await untouched(failed)).toBe('untouched')
    expect(await untouched(failed, { local: true })).toBe('untouched')
  })

  it('in an observer activation, closes an untouched OutputStream only for an undefined or null result (HTTP buffered rule)', async () => {
    const activation = { activation: true }
    expect(await untouched(settled(undefined), activation)).toBe('closed')
    expect(await untouched(settled(null), activation)).toBe('closed')
    expect(await untouched(settled({ status_code: 200 }), activation)).toBe('untouched')
    expect(await untouched(settled(0), activation)).toBe('untouched')
    expect(await untouched(failed, activation)).toBe('untouched')
  })

  it('skips an OutputStream kept open, ended, or sent on', async () => {
    const kept = collector()
    const out = OutputStream.to(kept.sink).keepOpen()
    const scope = new StreamScope()
    scope.adopt(out)
    await out.write('x')
    scope.settle(failed)
    await new Promise((r) => setTimeout(r, 10))
    expect(kept.sink.destroyed || kept.sink.writableEnded).toBe(false)

    const sent = outputFromRef(ref('write'), nowhere)
    const scope2 = new StreamScope()
    scope2.adopt(sent)
    markTransferred(sent)
    scope2.settle(settled(null))
    expect(isTransferred(sent)).toBe(true)
  })

  it('cancels an InputStream the handler started and did not finish; leaves one never read', async () => {
    let closedA = false
    const a = InputStream.from(
      (async function* () {
        try {
          for (;;) yield 'x'
        } finally {
          closedA = true
        }
      })(),
    )
    let pulledB = 0
    const b = InputStream.from(
      (async function* () {
        pulledB++
        yield 'y'
      })(),
    )
    const scope = new StreamScope()
    scope.adopt(a)
    scope.adopt(b)
    const it = a[Symbol.asyncIterator]()
    await it.next()
    scope.settle(settled(null))
    await new Promise((r) => setTimeout(r, 10))
    expect(closedA).toBe(true)
    expect(pulledB).toBe(0)
    expect(await b.text()).toBe('y')
  })

  it('in local mode, releases a source of this process nobody read', async () => {
    let released = false
    const source = new Readable({ read: () => undefined })
    source.on('close', () => {
      released = true
    })
    const input = InputStream.from(source)
    const scope = new StreamScope()
    scope.adopt(input)
    scope.settle(settled(null), { local: true })
    await new Promise((r) => setTimeout(r, 10))
    expect(released).toBe(true)
  })
})

describe('a local source that throws', () => {
  it('reaches the reader as a StreamAbortedError, with the error message as the reason', async () => {
    const input = InputStream.from(
      (async function* () {
        yield 'ab'
        throw new Error('local boom midway')
      })(),
    )
    const error = await input.text().catch((e: unknown) => e)
    expect(error).toBeInstanceOf(StreamAbortedError)
    expect(error).toMatchObject({ code: 'STREAM_ABORTED', reason: 'local boom midway' })
  })

  it('a Readable destroyed with an error aborts the stream the same way', async () => {
    const source = new Readable({ read: () => undefined })
    const reading = InputStream.from(source).bytes()
    source.push('x')
    source.destroy(new Error('disk gone'))
    await expect(reading).rejects.toMatchObject({ code: 'STREAM_ABORTED', reason: 'disk gone' })
  })
})

describe('error codes shared by the SDKs', () => {
  it('a second reader, a read of a stream sent away and tee() after a read are STREAM_IN_USE', async () => {
    const input = InputStream.from('data')
    await input.text()
    await expect(input.text()).rejects.toMatchObject({ code: 'STREAM_IN_USE' })
    expect(() => input.lines()).toThrow(expect.objectContaining({ code: 'STREAM_IN_USE' }))
    expect(() => input.tee()).toThrow(expect.objectContaining({ code: 'STREAM_IN_USE' }))

    const sent = inputFromRef(ref('read'), nowhere)
    markTransferred(sent)
    await expect(sent.bytes()).rejects.toMatchObject({ code: 'STREAM_IN_USE' })
    const sentOut = outputFromRef(ref('write'), nowhere)
    markTransferred(sentOut)
    await expect(sentOut.write('x')).rejects.toMatchObject({ code: 'STREAM_IN_USE' })
  })

  it('writing or sending after close or abort is STREAM_CLOSED', async () => {
    const closed = OutputStream.to(collector().sink)
    await closed.close()
    await expect(closed.write('x')).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
    await expect(closed.send({})).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
    const aborted = OutputStream.to(collector().sink)
    await aborted.abort('stop')
    await expect(aborted.write('x')).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
  })
})

describe('handOn (passing an InputStream on without its ref)', () => {
  it('passes a stream nobody read as it is', () => {
    const input = InputStream.from('x')
    expect(handOn(input)).toBe(input)
  })

  it('passes the unread rest of a stream a read began on', async () => {
    const input = InputStream.from(['head;', 'a', 'b'])
    const it = input[Symbol.asyncIterator]()
    expect((await it.next()).value?.toString()).toBe('head;')
    const rest = handOn(input)
    expect(rest).not.toBe(input)
    expect(await rest.text()).toBe('ab')
  })

  it('passes everything of a stream claimed but not started yet', async () => {
    const input = InputStream.from(['a', 'b'])
    input.lines()
    expect(await handOn(input).text()).toBe('ab')
  })

  it('refuses a stream read to its end, as reading it again would (R6)', async () => {
    const input = InputStream.from('all')
    await input.text()
    expect(() => handOn(input)).toThrow(
      expect.objectContaining({ code: 'STREAM_IN_USE', message: expect.stringMatching(/one reader/) }),
    )
  })
})

describe('holdsWriteSide', () => {
  it('finds an OutputStream, a native sink or a write ref, nested; not a read side', () => {
    expect(holdsWriteSide({ out: OutputStream.to(new Writable()) })).toBe(true)
    expect(holdsWriteSide([{ w: new Writable() }])).toBe(true)
    expect(holdsWriteSide({ a: { b: ref('write') } })).toBe(true)
    expect(holdsWriteSide({ body: ref('read'), input: InputStream.from('x') })).toBe(false)
    expect(holdsWriteSide({ n: 1 })).toBe(false)
  })
})

describe('forwarding (R5)', () => {
  it('an InputStream never read and an OutputStream never used travel as their ref', async () => {
    const input = inputFromRef(ref('read'), nowhere)
    expect(forwardableRef(input)).toEqual(ref('read'))
    const out = outputFromRef(ref('write'), nowhere)
    expect(forwardableRef(out)).toEqual(ref('write'))
    expect(forwardableRef(InputStream.from('x'))).toBeUndefined()
    markTransferred(input)
    expect(forwardableRef(input)).toBeUndefined()
    await expect(input.bytes()).rejects.toThrow(/sent to another function/)
    expect(() => input.lines()).toThrow(/sent to another function/)
  })
})

describe('the stream walk', () => {
  it('detects stream values and refs only in arrays and plain objects', () => {
    expect(streamKind(Readable.from([]))).toBe('source')
    expect(streamKind(new Writable())).toBe('sink')
    expect(streamKind(new Blob([]))).toBe('source')
    expect(
      streamKind(
        (async function* () {
          yield 'x'
        })(),
      ),
    ).toBe('source')
    expect(streamKind(new WritableStream())).toBe('sink')
    expect(streamKind({ pipe: () => undefined })).toBeUndefined()
    expect(containsStreams({ a: [{ b: InputStream.from('x') }] })).toBe(true)
    expect(containsStreams({ a: ref('read') })).toBe(true)
    expect(containsStreams(new Map([['a', InputStream.from('x')]]))).toBe(false)
    expect(containsStreams({ d: new Date(), n: 1 })).toBe(false)
  })

  it('mapStreams copies on write and keeps what has no stream', () => {
    const untouched = { keep: { a: 1 } }
    const value = { body: Readable.from([]), keep: untouched.keep, list: [1, ref('write')] }
    const mapped = mapStreams(value, (_slot, kind) => kind) as Record<string, unknown>
    expect(mapped).toEqual({ body: 'source', keep: { a: 1 }, list: [1, 'ref'] })
    expect(mapped.keep).toBe(untouched.keep)
    expect(mapStreams(untouched, () => 'x')).toBe(untouched)
  })

  it('wireRef keeps the three ref fields, no more', () => {
    expect(wireRef({ ...ref('read'), extra: 1 } as StreamChannelRef)).toEqual(ref('read'))
  })

  it('cloneKeepingStreams copies the values around streams and keeps the streams', () => {
    const input = InputStream.from('x')
    const value = { body: input, meta: { at: new Date(0) }, list: [1] }
    const copy = cloneKeepingStreams(value, structuredClone) as typeof value
    expect(copy).not.toBe(value)
    expect(copy.body).toBe(input)
    expect(copy.meta).not.toBe(value.meta)
    expect(copy.meta.at).toEqual(new Date(0))
  })

  it('asStream wraps a native source or sink', () => {
    expect(asStream(Readable.from([]))).toBeInstanceOf(InputStream)
    expect(asStream(new Writable())).toBeInstanceOf(OutputStream)
    const input = InputStream.from('x')
    expect(asStream(input)).toBe(input)
  })

  it('isObservedEnvelope matches the activation the payload arrived with', () => {
    const activation = { observer_id: 'o1', activation_id: 'a1' }
    expect(isObservedEnvelope({ observer_id: 'o1', activation_id: 'a1', input: {} }, activation)).toBe(true)
    expect(isObservedEnvelope({ observer_id: 'o1', activation_id: 'a2' }, activation)).toBe(false)
    expect(isObservedEnvelope({ observer_id: 'o1', activation_id: 'a1' }, undefined)).toBe(false)
    expect(isObservedEnvelope({ observer_id: 'o1', activation_id: 'a1' }, { observer_id: 'o1' })).toBe(false)
  })
})

describe('regressions from the adversarial run', () => {
  it('send() of a value JSON cannot serialize returns a rejected promise, it does not throw', async () => {
    const out = OutputStream.to(new WritableStream<Uint8Array>())
    let sent: Promise<void> | undefined
    expect(() => {
      sent = out.send(BigInt(1))
    }).not.toThrow()
    await expect(sent).rejects.toThrow(TypeError)
    const cycle: Record<string, unknown> = {}
    cycle.self = cycle
    await expect(out.send(cycle)).rejects.toThrow(TypeError)
    // A refused message is not a write: the stream goes on.
    await out.send({ ok: true })
    await out.close()
  })

  it('an abort passed on keeps its reason: no second "stream aborted" prefix', async () => {
    const original = new StreamAbortedError('channel writer aborted: closed with code 1011')
    const message = 'STREAM_ABORTED: stream aborted: channel writer aborted: closed with code 1011'
    const reasonOf = (s: OutputStream) => outputDone(s).catch((e: StreamAbortedError) => e)

    // A pump whose input was aborted aborts its sink (a WritableStream, a Writable) with the same abort.
    let webReason: unknown
    const web = OutputStream.to(
      new WritableStream<Uint8Array>({
        abort: (r) => {
          webReason = r
        },
      }),
    )
    const failing = () =>
      InputStream.from(
        (async function* () {
          yield 'x'
          throw original
        })(),
      )
    await expect(pumpStream(failing(), web)).rejects.toBe(original)
    expect((webReason as Error).message).toBe(message)
    expect(await reasonOf(web)).toMatchObject({ reason: original.reason, message })
    let destroyedWith: Error | undefined
    const writable = new Writable({
      write: (_c, _e, cb) => cb(),
      destroy: (error, cb) => {
        destroyedWith = error ?? undefined
        cb(null)
      },
    })
    const node = OutputStream.to(writable)
    await expect(pumpStream(failing(), node)).rejects.toBe(original)
    expect(destroyedWith?.message).toBe(message)

    // abort() with an abort, toWebStream().abort() with one, and the guard of a handler that failed with one.
    const aborted = OutputStream.to(new WritableStream<Uint8Array>())
    await aborted.abort(original)
    expect(await reasonOf(aborted)).toMatchObject({ reason: original.reason, message })
    const bridged = OutputStream.to(new WritableStream<Uint8Array>())
    await bridged.toWebStream().abort(original)
    expect(await reasonOf(bridged)).toMatchObject({ reason: original.reason, message })
    const guarded = OutputStream.to(new WritableStream<Uint8Array>())
    const scope = new StreamScope()
    scope.adopt(guarded)
    await guarded.write('x')
    scope.settle({ ok: false, error: original })
    expect(await reasonOf(guarded)).toMatchObject({ reason: original.reason, message })
  })
})

describe('a cancelled InputStream fails its reads with STREAM_CLOSED, it does not end', () => {
  const cancelled = { code: 'STREAM_CLOSED', message: 'STREAM_CLOSED: the stream was cancelled' }
  const guard = {
    code: 'STREAM_CLOSED',
    message: 'STREAM_CLOSED: the stream was cancelled when its handler returned: read it before returning',
  }
  const drain = async (items: AsyncIterable<unknown>) => {
    for await (const _ of items) {
      // reading
    }
  }
  const readers: Record<string, (s: InputStream) => Promise<unknown>> = {
    'for await': (s) => drain(s),
    'bytes()': (s) => s.bytes(),
    'text()': (s) => s.text(),
    'lines()': (s) => drain(s.lines()),
    'messages()': (s) => drain(s.messages()),
    'frames()': (s) => drain(s.frames()),
    'toWebStream()': async (s) => {
      const reader = s.toWebStream().getReader()
      while (!(await reader.read()).done) {
        // reading
      }
    },
    'a tee branch': (s) => {
      const [a, b] = s.tee()
      void b.cancel()
      return a.bytes()
    },
    'toReadable()': (s) => drain(s.toReadable()),
  }
  /** A source that sends one chunk, then waits forever: a read of it is in progress until it is cancelled. */
  const stalled = () =>
    InputStream.from(
      new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new TextEncoder().encode('first'))
        },
        pull: () => new Promise<void>(() => undefined),
      }),
    )
  const inProgress = (s: InputStream, read: (s: InputStream) => Promise<unknown>) => {
    const reading = read(s)
    reading.catch(() => undefined)
    return new Promise((r) => setTimeout(r, 20)).then(() => reading)
  }

  it.each(Object.keys(readers))('a read after cancel(): %s', async (name) => {
    const input = InputStream.from(['a', 'b'])
    await input.cancel()
    await expect(readers[name](input)).rejects.toMatchObject(cancelled)
  })

  it.each(Object.keys(readers))('a read in progress when cancel() lands: %s', async (name) => {
    const input = stalled()
    const reading = readers[name](input)
    reading.catch(() => undefined)
    await new Promise((r) => setTimeout(r, 20))
    await input.cancel()
    await expect(reading).rejects.toMatchObject(cancelled)
  })

  it.each(
    Object.keys(readers),
  )('a read the handler guard cut (the handler returned while it ran): %s', async (name) => {
    const input = stalled()
    const scope = new StreamScope()
    scope.adopt(input)
    const reading = readers[name](input)
    reading.catch(() => undefined)
    await new Promise((r) => setTimeout(r, 20))
    scope.settle({ ok: true, value: null })
    await expect(reading).rejects.toMatchObject(guard)
    void inProgress
  })

  it('a background task reading after its handler returned gets the guard error, not a short stream', async () => {
    const input = InputStream.from(Array.from({ length: 16 }, () => new Uint8Array(65_536)))
    const scope = new StreamScope()
    scope.adopt(input)
    const it = input[Symbol.asyncIterator]()
    await it.next()
    scope.settle({ ok: true, value: 'returned' })
    await expect(it.next()).rejects.toMatchObject(guard)
  })

  it('in local mode, a source nobody read is released and a later read fails with the guard error', async () => {
    const input = InputStream.from(['a'])
    const scope = new StreamScope()
    scope.adopt(input)
    scope.settle({ ok: true, value: null }, { local: true })
    await expect(input.text()).rejects.toMatchObject(guard)
  })

  it('a stream read to its end, or aborted, keeps its outcome after a cancel', async () => {
    const ended = InputStream.from(['a'])
    const it = ended[Symbol.asyncIterator]()
    expect((await it.next()).done).toBe(false)
    expect((await it.next()).done).toBe(true)
    await ended.cancel()
    const [a, b] = InputStream.from(['x']).tee()
    expect(await a.text()).toBe('x')
    await a.cancel()
    expect(await b.text()).toBe('x')
    const aborted = InputStream.from(
      (async function* () {
        yield 'a'
        throw new Error('boom')
      })(),
    )
    const reading = aborted.text()
    await expect(reading).rejects.toMatchObject({ code: 'STREAM_ABORTED', reason: 'boom' })
    await aborted.cancel()
  })

  it('a stream handed over (handOver) is not cancelled: the new one reads everything, the old one counts as sent', async () => {
    const input = InputStream.from(['a', 'b'])
    const relay = handOver(input)
    expect(relay).not.toBe(input)
    expect(await relay.text()).toBe('ab')
    await expect(input.bytes()).rejects.toMatchObject({ code: 'STREAM_IN_USE' })
    // One a read began on hands on its rest, as before.
    const partly = InputStream.from(['head;', 'tail'])
    const it = partly[Symbol.asyncIterator]()
    await it.next()
    expect(await handOver(partly).text()).toBe('tail')
  })

  it('a tee branch cancelled while its read waits fails that read; the other branch reads on', async () => {
    let more: (() => void) | undefined
    const gate = new Promise<void>((resolve) => {
      more = resolve
    })
    const [a, b] = InputStream.from(
      (async function* () {
        yield 'first;'
        await gate
        yield 'second'
      })(),
    ).tee()
    const readA = a.bytes()
    readA.catch(() => undefined)
    const readB = b.text()
    await new Promise((r) => setTimeout(r, 20))
    await a.cancel()
    await expect(readA).rejects.toMatchObject(cancelled)
    more?.()
    await expect(readB).resolves.toBe('first;second')
  })

  it('abortInput() fails a read in progress and later reads with STREAM_ABORTED', async () => {
    const input = stalled()
    const reading = input.bytes()
    reading.catch(() => undefined)
    await new Promise((r) => setTimeout(r, 20))
    await abortInput(input, 'the call timed out')
    await expect(reading).rejects.toMatchObject({ code: 'STREAM_ABORTED', reason: 'the call timed out' })
  })
})

describe('an abort does not wait for a write the sink holds', () => {
  /** A WritableStream whose writes never settle (a stuck consumer). */
  const stuck = () =>
    new WritableStream<Uint8Array>({ write: () => new Promise<void>(() => undefined) }, { highWaterMark: 1 })

  it('abort() returns at once and the held writes fail with STREAM_CLOSED (a WritableStream)', async () => {
    const out = OutputStream.to(stuck())
    const first = out.write('a')
    const second = out.write('b')
    first.catch(() => undefined)
    second.catch(() => undefined)
    await new Promise((r) => setTimeout(r, 20))
    const started = performance.now()
    await out.abort('stop')
    expect(performance.now() - started).toBeLessThan(100)
    await expect(first).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
    await expect(second).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
    await expect(outputDone(out)).rejects.toMatchObject({ code: 'STREAM_ABORTED', reason: 'stop' })
  })

  it('a pump into a stuck sink stops when the sink is aborted', async () => {
    const out = OutputStream.to(stuck())
    const pump = pumpStream(InputStream.from(['a', 'b', 'c']), out)
    pump.catch(() => undefined)
    await new Promise((r) => setTimeout(r, 20))
    await out.abort('stop')
    await expect(pump).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
  })

  it('abort() returns at once and a write held by a Writable that never drains fails with STREAM_CLOSED', async () => {
    const out = OutputStream.to(new Writable({ highWaterMark: 1, write: () => undefined }))
    const held = out.write('a large enough chunk')
    held.catch(() => undefined)
    await new Promise((r) => setTimeout(r, 20))
    const started = performance.now()
    await out.abort('stop')
    expect(performance.now() - started).toBeLessThan(100)
    await expect(held).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
  })
})
