import { afterEach, describe, expect, it } from 'vitest'
import { WebSocket } from 'ws'
import { inputFromRef, outputFromRef } from '../../src/call-streams'
import type { StreamChannelRef } from '../../src/iii-types'
import { CallAction, InputStream, OutputStream, StreamAbortedError } from '../../src/index'
import type { Observer } from '../../src/observe-types'
import type { ISdk } from '../../src/types'
import { closeWorkers, collector, localDispatchReady, sleep, waitFor, worker } from './observe-utils'
import { engineWsUrl } from './utils'

/**
 * Call-native streams against a real engine (`III_URL`): a host worker runs the
 * functions and a caller worker calls them, so every call and every pipe goes
 * through the engine. The browser socket is `ws`'s WebSocket (utils.ts), with
 * the browser's API.
 */
afterEach(closeWorkers)

const RUN = Math.random().toString(16).slice(2, 10)
const fid = (name: string) => `browser-streams::${name}-${RUN}`
const encode = (s: string) => new TextEncoder().encode(s)
const decode = (b: Uint8Array) => new TextDecoder().decode(b)

/** Two workers: the host registers `functions`, the caller calls them. */
async function pair(functions: (host: ISdk, caller: ISdk) => void): Promise<{ host: ISdk; caller: ISdk }> {
  const host = worker('streams-host')
  const caller = worker('streams-caller')
  functions(host, caller)
  await sleep(300)
  return { host, caller }
}

/** A raw pipe of `sdk` (`engine::channels::create` sent as it is): what an old SDK builds by hand. */
function rawPipe(sdk: ISdk): Promise<{ writer: StreamChannelRef; reader: StreamChannelRef }> {
  return (
    sdk as unknown as { createPipeRefs(): Promise<{ writer: StreamChannelRef; reader: StreamChannelRef }> }
  ).createPipeRefs()
}

/** `p`, or a rejection after `ms`: a call that never ends fails the test instead of hanging it. */
function within<T>(p: Promise<T>, ms = 5000): Promise<T> {
  return Promise.race([
    p,
    sleep(ms).then(() => {
      throw new Error(`no answer within ${ms} ms (circular wait?)`)
    }),
  ]) as Promise<T>
}

/** A WritableStream that records what it gets, and how it ended. */
function collectorSink(): {
  sink: WritableStream<Uint8Array>
  text: () => string
  state: () => 'open' | 'closed' | 'aborted'
} {
  const chunks: Uint8Array[] = []
  let state: 'open' | 'closed' | 'aborted' = 'open'
  const sink = new WritableStream<Uint8Array>({
    write(chunk) {
      chunks.push(chunk)
    },
    close() {
      state = 'closed'
    },
    abort() {
      state = 'aborted'
    },
  })
  return { sink, text: () => chunks.map(decode).join(''), state: () => state }
}

/** A ReadableStream of `chunks`. */
function readableOf(chunks: Array<string | Uint8Array>): ReadableStream<Uint8Array> {
  return new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(typeof chunk === 'string' ? encode(chunk) : chunk)
      controller.close()
    },
  })
}

describe('call-native streams through the engine (browser SDK)', () => {
  it('send: a ReadableStream, a Blob, a File and an async generator in the payload reach the handler as an InputStream', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('count'), async ({ body }: { body: InputStream }) => {
        expect(body).toBeInstanceOf(InputStream)
        let bytes = 0
        for await (const chunk of body) bytes += chunk.byteLength
        return { bytes }
      })
    })
    const data = new Uint8Array(300_000).fill(7)
    await expect(
      caller.call({
        function_id: fid('count'),
        payload: { body: readableOf([data.subarray(0, 1000), data.subarray(1000)]) },
      }),
    ).resolves.toEqual({ bytes: 300_000 })
    await expect(caller.call({ function_id: fid('count'), payload: { body: new Blob(['abc']) } })).resolves.toEqual({
      bytes: 3,
    })
    await expect(
      caller.call({ function_id: fid('count'), payload: { body: new File(['abcd'], 'upload.txt') } }),
    ).resolves.toEqual({ bytes: 4 })
    async function* gen() {
      yield 'ab'
      yield encode('cde')
    }
    await expect(caller.call({ function_id: fid('count'), payload: { body: gen() } })).resolves.toEqual({ bytes: 5 })
  })

  it('return: an async generator in the result reaches the caller as an InputStream', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('export'), async ({ rows }: { rows: number }) => {
        async function* lines() {
          yield 'id,total\n'
          for (let i = 0; i < rows; i++) yield `${i},${i * 2}\n`
        }
        return { csv: lines(), rows }
      })
    })
    const result = await caller.call<unknown, { csv: InputStream; rows: number }>({
      function_id: fid('export'),
      payload: { rows: 500 },
    })
    expect(result.rows).toBe(500)
    expect(result.csv).toBeInstanceOf(InputStream)
    const lines: string[] = []
    for await (const line of result.csv.lines()) lines.push(line)
    expect(lines).toHaveLength(501)
    expect(lines[lines.length - 1]).toBe('499,998')
  })

  it('return: a ReadableStream in the result reads through toWebStream()', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('web-out'), async () => ({ body: readableOf(['hello ', 'web']) }))
    })
    const { body } = await caller.call<unknown, { body: InputStream }>({ function_id: fid('web-out'), payload: {} })
    expect(await new Response(body.toWebStream()).text()).toBe('hello web')
  })

  it('sink: a WritableStream in the payload; the call resolves once the sink finished (R4)', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('render'), async ({ out, pages }: { out: OutputStream; pages: number }) => {
        expect(out).toBeInstanceOf(OutputStream)
        for (let i = 0; i < pages; i++) await out.write(`page-${i};`)
      })
    })
    const { sink, text, state } = collectorSink()
    await caller.call({ function_id: fid('render'), payload: { out: sink, pages: 30 } })
    expect(state()).toBe('closed')
    expect(text().split(';')).toHaveLength(31)
  })

  it('abort: a handler that throws while writing aborts the sink, and the call rejects with the error body', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('fail-writing'), async ({ out }: { out: OutputStream }) => {
        await out.write('partial')
        throw new Error('render failed')
      })
    })
    const { sink, state } = collectorSink()
    await expect(caller.call({ function_id: fid('fail-writing'), payload: { out: sink } })).rejects.toMatchObject({
      message: expect.stringMatching(/render failed/),
    })
    await waitFor(() => state() === 'aborted', 'the sink aborted')
  })

  it('abort: a source that throws reaches the reader as STREAM_ABORTED after the data before it', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('read-all'), async ({ body }: { body: InputStream }) => {
        const chunks: string[] = []
        try {
          for await (const chunk of body) chunks.push(decode(chunk))
        } catch (error) {
          return { chunks, code: (error as StreamAbortedError).code, isAbort: error instanceof StreamAbortedError }
        }
        return { chunks }
      })
    })
    async function* failing() {
      yield 'first'
      await sleep(30)
      throw new Error('disk read failed')
    }
    await expect(caller.call({ function_id: fid('read-all'), payload: { body: failing() } })).resolves.toEqual({
      chunks: ['first'],
      code: 'STREAM_ABORTED',
      isAbort: true,
    })
  })

  it('abort: out.abort() in the handler rejects the call with STREAM_ABORTED; the engine took close code 4011 as an abort', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('abort-out'), async ({ out }: { out: OutputStream }) => {
        await out.write('x')
        await out.abort('bad input')
        return 'returned anyway'
      })
    })
    const { sink } = collectorSink()
    const error = await caller.call({ function_id: fid('abort-out'), payload: { out: sink } }).catch((e: unknown) => e)
    expect(error).toBeInstanceOf(StreamAbortedError)
    expect(error).toMatchObject({ code: 'STREAM_ABORTED', function_id: fid('abort-out') })
    expect((error as StreamAbortedError).reason).toBe('channel writer aborted: closed with code 4011')
  })

  it('abort: a writer whose connection drops without a close frame aborts the reader', async () => {
    const solo = worker('streams-writer-drop')
    await sleep(200)
    const pipe = await rawPipe(solo)
    const read = inputFromRef(pipe.reader, { wsBase: engineWsUrl }).bytes()
    read.catch(() => undefined)

    const { channel_id, access_key } = pipe.writer
    const writer = new WebSocket(
      `${engineWsUrl}/ws/channels/${channel_id}?key=${encodeURIComponent(access_key)}&dir=write`,
    )
    await new Promise((resolve, reject) => {
      writer.once('open', resolve)
      writer.once('error', reject)
    })
    writer.send(encode('partial'))
    await sleep(100)
    writer.terminate()

    await expect(read).rejects.toMatchObject({
      code: 'STREAM_ABORTED',
      reason: 'channel writer aborted: connection lost before close',
    })
  })

  it('forward: a handler passes an unread InputStream on, and its ref travels (R5)', async () => {
    const { caller } = await pair((host, other) => {
      other.registerFunction(fid('count-bytes'), async ({ body }: { body: InputStream }) => ({
        bytes: (await body.bytes()).byteLength,
      }))
      host.registerFunction(fid('forward'), async ({ body }: { body: InputStream }) =>
        host.call({ function_id: fid('count-bytes'), payload: { body } }),
      )
    })
    await expect(
      caller.call({ function_id: fid('forward'), payload: { body: InputStream.from(new Uint8Array(70_000).fill(1)) } }),
    ).resolves.toEqual({ bytes: 70_000 })
  })

  it('guard: an input the handler started and left is cancelled, which stops the source', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('partial'), async ({ body }: { body: InputStream }) => {
        const it = body[Symbol.asyncIterator]()
        await it.next()
        return 'read one chunk'
      })
    })
    let cancelled = false
    const endless = new ReadableStream<Uint8Array>({
      pull(controller) {
        controller.enqueue(new Uint8Array(64 * 1024).fill(1))
      },
      cancel() {
        cancelled = true
      },
    })
    await expect(caller.call({ function_id: fid('partial'), payload: { body: endless } })).resolves.toBe(
      'read one chunk',
    )
    await waitFor(() => cancelled, 'the source cancelled')
  })

  it('guard (v2): a plain call closes an untouched OutputStream whatever the result, so a sink call resolves', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('buffered'), async (_input: { out: OutputStream }) => ({ status_code: 200 }))
      host.registerFunction(fid('nothing'), async (_input: { out: OutputStream }) => null)
      host.registerFunction(fid('fails'), async (_input: { out: OutputStream }) => {
        throw new Error('nothing written')
      })
    })
    const { sink, text, state } = collectorSink()
    const started = Date.now()
    await expect(caller.call({ function_id: fid('buffered'), payload: { out: sink } })).resolves.toEqual({
      status_code: 200,
    })
    expect(Date.now() - started).toBeLessThan(5000)
    expect(state()).toBe('closed')
    expect(text()).toBe('')

    for (const [function_id, expected] of [
      [fid('buffered'), { status_code: 200 }],
      [fid('nothing'), null],
    ] as const) {
      const pipe = await rawPipe(caller)
      await expect(caller.call({ function_id, payload: { out: pipe.writer } })).resolves.toEqual(expected)
      await expect(inputFromRef(pipe.reader, { wsBase: engineWsUrl }).bytes()).resolves.toHaveLength(0)
    }

    // On failure the untouched stream is left alone: the caller learns from the error.
    const failed = await rawPipe(caller)
    await expect(caller.call({ function_id: fid('fails'), payload: { out: failed.writer } })).rejects.toMatchObject({
      message: expect.stringMatching(/nothing written/),
    })
    const failedInput = inputFromRef(failed.reader, { wsBase: engineWsUrl })
    const race = await Promise.race([failedInput.bytes().then(() => 'ended'), sleep(500).then(() => 'open')])
    expect(race).toBe('open')
    void failedInput.cancel()
  })

  it('guard: an observer activation leaves an untouched OutputStream open for a non-null result, and closes it for null', async () => {
    const source = worker('streams-route-source')
    const routes = worker('streams-route-target')
    let observer: Observer | undefined
    source
      .registerFunction(fid('on-request'), async (request: unknown) => request)
      .onObserverAdded((obs) => {
        observer = obs
      })
    routes.registerFunction(fid('route'), async ({ buffered }: { out: OutputStream; buffered: boolean }) =>
      buffered ? { status_code: 200, body: 'buffered' } : null,
    )
    const handle = routes.observe({ function_id: fid('on-request') }, fid('route'))
    await handle.ready()
    const obs = await waitFor(() => observer, 'the observer reached its source')
    await obs.accepted()

    const kept = await rawPipe(source)
    await expect(source.call({ observer: obs, payload: { out: kept.writer, buffered: true } })).resolves.toEqual({
      status_code: 200,
      body: 'buffered',
    })
    const keptInput = inputFromRef(kept.reader, { wsBase: engineWsUrl })
    const race = await Promise.race([keptInput.bytes().then(() => 'ended'), sleep(500).then(() => 'open')])
    expect(race).toBe('open')
    void keptInput.cancel()

    const closed = await rawPipe(source)
    await expect(source.call({ observer: obs, payload: { out: closed.writer, buffered: false } })).resolves.toBeNull()
    await expect(inputFromRef(closed.reader, { wsBase: engineWsUrl }).bytes()).resolves.toHaveLength(0)
    await handle.dispose()
  })

  it('guard: a used OutputStream with a value result is closed', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('wrote'), async ({ out }: { out: OutputStream }) => {
        await out.write('body')
        return { ok: true }
      })
    })
    const pipe = await rawPipe(caller)
    const read = inputFromRef(pipe.reader, { wsBase: engineWsUrl }).text()
    await expect(caller.call({ function_id: fid('wrote'), payload: { out: pipe.writer } })).resolves.toEqual({
      ok: true,
    })
    await expect(read).resolves.toBe('body')
  })

  it('one reader (R6): a second read in the handler throws', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('read-twice'), async ({ body }: { body: InputStream }) => {
        const first = await body.text()
        const second = await body.text().catch((e: Error) => e.message)
        return { first, second }
      })
    })
    const result = await caller.call<unknown, { first: string; second: string }>({
      function_id: fid('read-twice'),
      payload: { body: InputStream.from('once') },
    })
    expect(result.first).toBe('once')
    expect(result.second).toMatch(/one reader/)
  })

  it('messages and bytes keep their order, through a pipe()', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('progress'), async ({ out }: { out: OutputStream }) => {
        await out.send({ type: 'progress', n: 1 })
        await out.write('bytes-1')
        await out.send({ type: 'progress', n: 2 })
        await out.write('bytes-2')
      })
    })
    const pipe = await caller.pipe()
    const call = caller.call({ function_id: fid('progress'), payload: { out: pipe.output } })
    const frames: string[] = []
    for await (const f of pipe.input.frames()) frames.push(f.type === 'text' ? `t:${f.value}` : `b:${decode(f.value)}`)
    await call
    expect(frames).toEqual(['t:{"type":"progress","n":1}', 'b:bytes-1', 't:{"type":"progress","n":2}', 'b:bytes-2'])
  })

  it('pipe(): the input end in a payload, written with pipeTo() after the call started', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('upper'), async ({ body }: { body: InputStream }) => (await body.text()).toUpperCase())
    })
    const pipe = await caller.pipe({ bufferSize: 8 })
    const result = caller.call({ function_id: fid('upper'), payload: { body: pipe.input } })
    await readableOf(['ab', 'cd']).pipeTo(pipe.output.toWebStream())
    await expect(result).resolves.toBe('ABCD')
  })

  it('Enqueue with a stream is rejected with STREAM_NOT_ALLOWED (R9); Void is allowed', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('sink-void'), async ({ body }: { body: InputStream }) => {
        await body.bytes()
      })
    })
    await expect(
      caller.call({
        function_id: fid('sink-void'),
        payload: { body: InputStream.from('x') },
        action: CallAction.Enqueue({ queue: 'default' }),
      }),
    ).rejects.toMatchObject({ code: 'STREAM_NOT_ALLOWED' })
    await expect(
      caller.call({
        function_id: fid('sink-void'),
        payload: { refs: [(await rawPipe(caller)).reader] },
        action: CallAction.Enqueue({ queue: 'default' }),
      }),
    ).rejects.toMatchObject({ code: 'STREAM_NOT_ALLOWED' })
    await expect(
      caller.call({
        function_id: fid('sink-void'),
        payload: { body: InputStream.from('x') },
        action: CallAction.Void(),
      }),
    ).resolves.toBeUndefined()
  })

  it('interop: a raw ref built by hand (what an old SDK sends) is read as an InputStream', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('raw-count'), async ({ body }: { body: InputStream }) => ({
        bytes: (await body.bytes()).byteLength,
        isStream: body instanceof InputStream,
      }))
    })
    const pipe = await rawPipe(caller)
    const result = caller.call({ function_id: fid('raw-count'), payload: { body: pipe.reader } })
    const out = outputFromRef(pipe.writer, { wsBase: engineWsUrl })
    await out.write(new Uint8Array(1234))
    await out.close()
    await expect(result).resolves.toEqual({ bytes: 1234, isStream: true })
  })

  it('local dispatch: streams reach a function of the same worker as streams, with no pipe', async () => {
    const solo = worker('streams-local')
    solo.registerFunction(fid('local'), async ({ body, out }: { body: InputStream; out: OutputStream }) => {
      expect(body).toBeInstanceOf(InputStream)
      expect(out).toBeInstanceOf(OutputStream)
      const text = await body.text()
      await out.write(text.toUpperCase())
      async function* tail() {
        yield 'tail'
      }
      return { len: text.length, tail: tail() }
    })
    await localDispatchReady(solo, fid('local'))
    const pipes: string[] = []
    const internal = solo as unknown as { createPipeRefs: () => Promise<unknown> }
    const create = internal.createPipeRefs.bind(solo)
    internal.createPipeRefs = () => {
      pipes.push('created')
      return create()
    }
    const { sink, text } = collectorSink()
    const result = await solo.call<unknown, { len: number; tail: InputStream }>({
      function_id: fid('local'),
      payload: { body: readableOf(['ab', 'cd']), out: sink, meta: { at: new Date(0) } },
    })
    expect(result.len).toBe(4)
    expect(text()).toBe('ABCD')
    expect(await result.tail.text()).toBe('tail')
    expect(pipes).toEqual([])
  })

  it('StreamInfo: an observer of a call that carried streams sees a StreamInfo, never a ref', async () => {
    const owner = worker('streams-observed')
    const watcher = worker('streams-watcher')
    owner.registerFunction(fid('observed'), async ({ body }: { body: InputStream }) => {
      async function* echo() {
        yield await body.bytes()
      }
      return { copy: echo() }
    })
    const got = collector(watcher, fid('on-observed'))
    const handle = watcher.observe({ function_id: fid('observed'), capture: ['input', 'output'] }, fid('on-observed'))
    await handle.ready()
    await sleep(200)

    const result = await watcher.call<unknown, { copy: InputStream }>({
      function_id: fid('observed'),
      payload: { body: InputStream.from('seen once') },
    })
    expect(await result.copy.text()).toBe('seen once')
    await waitFor(() => got.length >= 1, 'one activation')
    const { input, output } = got[0].data as { input: { body: unknown }; output: { copy: unknown } }
    expect(input.body).toEqual({ stream: true, direction: 'read', id: expect.any(String) })
    expect(output.copy).toEqual({ stream: true, direction: 'read', id: expect.any(String) })
    await handle.dispose()
  })

  it('the call timeout bounds the wait for a sink the handler kept open: TIMEOUT from the call start, sink aborted (routed and local)', async () => {
    const keepOpen = async ({ out }: { out: OutputStream }) => {
      await sleep(1000)
      await out.write('started;')
      out.keepOpen()
      return { ok: true }
    }
    const { caller } = await pair((host) => {
      host.registerFunction(fid('keep-open'), keepOpen)
    })
    const solo = worker('streams-local-keep-open')
    solo.registerFunction(fid('local-keep-open'), keepOpen)
    await localDispatchReady(solo, fid('local-keep-open'))
    for (const [sdk, function_id] of [
      [caller, fid('keep-open')],
      [solo, fid('local-keep-open')],
    ] as const) {
      const { sink, state } = collectorSink()
      const started = performance.now()
      const error = await within(sdk.call({ function_id, payload: { out: sink }, timeoutMs: 1500 })).catch(
        (e: unknown) => e,
      )
      const elapsed = performance.now() - started
      expect(error).toMatchObject({ code: 'TIMEOUT', function_id })
      expect(elapsed).toBeGreaterThanOrEqual(1400)
      expect(elapsed).toBeLessThan(2200)
      await waitFor(() => state() === 'aborted', 'the sink aborted')
    }
  })

  it('a sink passed back in the result is not waited for: the caller writes through it (routed and local)', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('pass-back'), async ({ out }: { out: OutputStream }) => ({ out }))
      host.registerFunction(fid('write-then-pass-back'), async ({ out }: { out: OutputStream }) => {
        await out.write('first;')
        return { out }
      })
    })
    caller.registerFunction(fid('local-pass-back'), async ({ out }: { out: OutputStream }) => ({ out }))
    await localDispatchReady(caller, fid('local-pass-back'))
    for (const [function_id, before] of [
      [fid('pass-back'), ''],
      [fid('write-then-pass-back'), 'first;'],
      [fid('local-pass-back'), ''],
    ] as const) {
      const { sink, text, state } = collectorSink()
      const result = await within(caller.call<unknown, { out: OutputStream }>({ function_id, payload: { out: sink } }))
      expect(result.out).toBeInstanceOf(OutputStream)
      await result.out.write('back')
      await result.out.close()
      await waitFor(() => state() === 'closed', 'the sink finished')
      expect(text()).toBe(`${before}back`)
    }
  })

  it('a partially read InputStream forwarded on hands over the unread rest (routed and local)', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('count-rest'), async ({ body }: { body: InputStream }) => ({ text: await body.text() }))
    })
    caller.registerFunction(fid('local-count-rest'), async ({ body }: { body: InputStream }) => ({
      text: await body.text(),
    }))
    caller.registerFunction(fid('forward-rest'), async ({ body, to }: { body: InputStream; to: string }) => {
      const it = body[Symbol.asyncIterator]()
      const head = decode((await it.next()).value as Uint8Array)
      const rest = await caller.call<unknown, { text: string }>({ function_id: to, payload: { body } })
      return { head, rest: rest.text }
    })
    await localDispatchReady(caller, fid('local-count-rest'))
    await localDispatchReady(caller, fid('forward-rest'))
    for (const to of [fid('count-rest'), fid('local-count-rest')]) {
      await expect(
        caller.call({ function_id: fid('forward-rest'), payload: { body: readableOf(['head;', 'a', 'b']), to } }),
      ).resolves.toEqual({ head: 'head;', rest: 'ab' })
    }
  })

  it('forwarding an InputStream read to its end throws the one-reader error, routed and local (R6)', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(
        fid('count-any'),
        async ({ body }: { body: InputStream }) => (await body.bytes()).byteLength,
      )
    })
    caller.registerFunction(
      fid('local-count-any'),
      async ({ body }: { body: InputStream }) => (await body.bytes()).byteLength,
    )
    await localDispatchReady(caller, fid('local-count-any'))
    for (const function_id of [fid('count-any'), fid('local-count-any')]) {
      const body = InputStream.from('read once')
      await body.text()
      await expect(caller.call({ function_id, payload: { body } })).rejects.toMatchObject({
        code: 'STREAM_IN_USE',
        message: expect.stringMatching(/one reader/),
      })
    }
  })

  it('the same stream twice in one payload, or a stream already sent, is STREAM_IN_USE (routed and local)', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('take-any'), async (_input: unknown) => 'ran')
    })
    caller.registerFunction(fid('local-take-any'), async (_input: unknown) => 'ran')
    await localDispatchReady(caller, fid('local-take-any'))
    for (const function_id of [fid('take-any'), fid('local-take-any')]) {
      const readable = readableOf(['x'])
      await expect(caller.call({ function_id, payload: { a: readable, b: readable } })).rejects.toMatchObject({
        code: 'STREAM_IN_USE',
        message: expect.stringMatching(/appears twice/),
      })
      const { sink } = collectorSink()
      await expect(caller.call({ function_id, payload: { o1: sink, o2: sink } })).rejects.toMatchObject({
        code: 'STREAM_IN_USE',
      })
    }
    // A pipe end sent unread travels as its ref: sending it again is refused, routed and local.
    const pipe = await caller.pipe()
    await caller.call({ function_id: fid('take-any'), payload: { body: pipe.input } })
    for (const function_id of [fid('take-any'), fid('local-take-any')]) {
      await expect(caller.call({ function_id, payload: { body: pipe.input } })).rejects.toMatchObject({
        code: 'STREAM_IN_USE',
        message: expect.stringMatching(/already sent/),
      })
    }
    void pipe.output.close()
  })

  it('an input read in part and returned in the result reaches the caller as its unread rest (routed and local)', async () => {
    const returnRest = async ({ body }: { body: InputStream }) => {
      const it = body[Symbol.asyncIterator]()
      const head = decode((await it.next()).value as Uint8Array)
      return { head, rest: body }
    }
    const { caller } = await pair((host) => {
      host.registerFunction(fid('return-rest'), returnRest)
    })
    caller.registerFunction(fid('local-return-rest'), returnRest)
    await localDispatchReady(caller, fid('local-return-rest'))
    const tail = Array.from({ length: 20 }, (_, i) => new Uint8Array(10_000).fill(i + 1))
    const expected = new Uint8Array(200_000)
    for (const [i, chunk] of tail.entries()) expected.set(chunk, i * 10_000)
    for (const function_id of [fid('return-rest'), fid('local-return-rest')]) {
      const result = await within(
        caller.call<unknown, { head: string; rest: InputStream }>({
          function_id,
          payload: { body: readableOf([encode('head;'), ...tail]) },
        }),
      )
      expect(result.head).toBe('head;')
      expect(result.rest).toBeInstanceOf(InputStream)
      // Read after the handler's guard ran: the rest must not have been cancelled by it.
      await sleep(100)
      expect(await within(result.rest.bytes())).toEqual(expected)
    }
  })

  it('tee(): two functions read the same input concurrently', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(
        fid('tee-count'),
        async ({ body }: { body: InputStream }) => (await body.bytes()).byteLength,
      )
    })
    const [a, b] = InputStream.from(readableOf(Array.from({ length: 50 }, () => new Uint8Array(4096)))).tee()
    await expect(
      within(
        Promise.all([
          caller.call({ function_id: fid('tee-count'), payload: { body: a } }),
          caller.call({ function_id: fid('tee-count'), payload: { body: b } }),
        ]),
      ),
    ).resolves.toEqual([204_800, 204_800])
  })

  it('local dispatch: a returned source that throws midway reaches the reader as STREAM_ABORTED', async () => {
    const solo = worker('streams-local-abort')
    solo.registerFunction(fid('local-throw-midway'), async () => ({
      out: (async function* () {
        yield encode('ab')
        throw new Error('local boom midway')
      })(),
    }))
    await localDispatchReady(solo, fid('local-throw-midway'))
    const { out } = await solo.call<unknown, { out: InputStream }>({
      function_id: fid('local-throw-midway'),
      payload: {},
    })
    const chunks: string[] = []
    const error = await (async () => {
      for await (const chunk of out) chunks.push(decode(chunk))
    })().catch((e: unknown) => e)
    expect(chunks).toEqual(['ab'])
    expect(error).toBeInstanceOf(StreamAbortedError)
    expect(error).toMatchObject({ code: 'STREAM_ABORTED', reason: 'local boom midway' })
  })
})

describe('refusals claim nothing (regressions from the adversarial run)', () => {
  it('the same stream twice in a nested local call is refused before anything is adopted: the outer call resolves and its sink closes', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('x09-b'), async (_input: unknown) => 'b')
      host.registerFunction(fid('x09-a'), async ({ out }: { out: OutputStream }) => {
        try {
          await host.call({ function_id: fid('x09-b'), payload: { a: out, b: out } })
          return 'ok'
        } catch (error) {
          return (error as { code?: string }).code
        }
      })
    })
    const { sink, state } = collectorSink()
    await expect(
      within(caller.call({ function_id: fid('x09-a'), payload: { out: sink }, timeoutMs: 4000 }), 3000),
    ).resolves.toBe('STREAM_IN_USE')
    await waitFor(() => state() === 'closed', 'the sink closed')
  })

  it('a partly read input twice in a call is refused before it leaves the guard, which still cancels it (routed and local)', async () => {
    const { host, caller } = await pair((host, other) => {
      other.registerFunction(fid('x09-take'), async (_input: unknown) => 'taken')
      host.registerFunction(fid('x09-local-take'), async (_input: unknown) => 'taken')
      host.registerFunction(fid('x09-dup-input'), async ({ body, to }: { body: InputStream; to: string }) => {
        const it = body[Symbol.asyncIterator]()
        await it.next()
        try {
          await host.call({ function_id: to, payload: { a: body, b: body } })
          return 'ok'
        } catch (error) {
          return (error as { code?: string }).code
        }
      })
    })
    await localDispatchReady(host, fid('x09-local-take'))
    for (const to of [fid('x09-take'), fid('x09-local-take')]) {
      let cancelled = false
      const endless = new ReadableStream<Uint8Array>({
        pull(controller) {
          controller.enqueue(new Uint8Array(16 * 1024).fill(1))
        },
        cancel() {
          cancelled = true
        },
      })
      await expect(
        within(caller.call({ function_id: fid('x09-dup-input'), payload: { body: endless, to } })),
      ).resolves.toBe('STREAM_IN_USE')
      await waitFor(() => cancelled, `the source cancelled (${to})`)
    }
  })

  it('a stream in metadata is refused before anything is sent, as the Rust SDK does (routed and local)', async () => {
    let ran = 0
    const take = async ({ body }: { body: InputStream }) => {
      ran++
      return body.text()
    }
    const { caller } = await pair((host) => {
      host.registerFunction(fid('meta'), take)
    })
    caller.registerFunction(fid('local-meta'), take)
    await localDispatchReady(caller, fid('local-meta'))
    for (const function_id of [fid('meta'), fid('local-meta')]) {
      for (const metadata of [
        { s: readableOf(['m']) },
        { list: [collectorSink().sink] },
        { nested: { input: InputStream.from('m'), file: new Blob(['m']) } },
      ]) {
        const body = readableOf(['payload'])
        await expect(caller.call({ function_id, payload: { body }, metadata: metadata as never })).rejects.toThrow(
          'a stream cannot travel in metadata: put it in the payload',
        )
        // Nothing was claimed: the same source goes in the next call.
        await expect(caller.call({ function_id, payload: { body } })).resolves.toBe('payload')
      }
    }
    expect(ran).toBe(6)
  })
})

describe('cancelled, aborted and sent streams (regressions from the Go adversarial run)', () => {
  const guard = {
    code: 'STREAM_CLOSED',
    message: 'STREAM_CLOSED: the stream was cancelled when its handler returned: read it before returning',
  }
  /** 16 chunks of 64 KiB (1 MiB), one every 5 ms: a read of it is in progress when its handler returns. */
  const slowMiB = () => {
    let sent = 0
    return new ReadableStream<Uint8Array>({
      pull: async (controller) => {
        await sleep(5)
        if (sent++ < 16) controller.enqueue(new Uint8Array(65_536).fill(1))
        else controller.close()
      },
    })
  }

  it('a background read after its handler returned fails with the guard error, never a short clean end (routed and local)', async () => {
    const results: Record<string, unknown> = {}
    const handler =
      (key: string) =>
      async ({ body }: { body: InputStream }) => {
        const it = body[Symbol.asyncIterator]()
        let bytes = ((await it.next()).value as Uint8Array).byteLength
        // Not awaited: it reads on after the handler returned.
        void (async () => {
          try {
            for (;;) {
              const step = await it.next()
              if (step.done) {
                results[key] = { bytes, ended: true }
                return
              }
              bytes += step.value.byteLength
            }
          } catch (error) {
            results[key] = { bytes, code: (error as { code?: string }).code, message: (error as Error).message }
          }
        })()
        return 'returned'
      }
    const { caller } = await pair((host) => {
      host.registerFunction(fid('bg-read'), handler('routed'))
    })
    caller.registerFunction(fid('local-bg-read'), handler('local'))
    await localDispatchReady(caller, fid('local-bg-read'))
    for (const [function_id, key] of [
      [fid('bg-read'), 'routed'],
      [fid('local-bg-read'), 'local'],
    ] as const) {
      await expect(within(caller.call({ function_id, payload: { body: slowMiB() } }))).resolves.toBe('returned')
      const result = (await waitFor(() => results[key], `the background read (${key})`)) as { bytes: number }
      expect(result).toMatchObject(guard)
      expect(result.bytes).toBeLessThan(1_048_576)
    }
  })

  it('a call that times out aborts the sources it passed: the handler read fails with STREAM_ABORTED (routed and local)', async () => {
    const seen: Record<string, unknown> = {}
    const slowReader =
      (key: string) =>
      async ({ body }: { body: InputStream }) => {
        try {
          for await (const _ of body) {
            // reading
          }
          seen[key] = 'ended'
        } catch (error) {
          seen[key] = (error as { code?: string }).code
        }
        return 'done'
      }
    const { caller } = await pair((host) => {
      host.registerFunction(fid('slow-read'), slowReader('routed'))
    })
    caller.registerFunction(fid('local-slow-read'), slowReader('local'))
    await localDispatchReady(caller, fid('local-slow-read'))
    for (const [function_id, key] of [
      [fid('slow-read'), 'routed'],
      [fid('local-slow-read'), 'local'],
    ] as const) {
      // Slow, so little is buffered on the way: across a pipe the abort comes after the data the engine holds.
      const endless = new ReadableStream<Uint8Array>({
        pull: async (controller) => {
          await sleep(20)
          controller.enqueue(new Uint8Array(1024).fill(2))
        },
      })
      await expect(
        within(caller.call({ function_id, payload: { body: endless }, timeoutMs: 600 })),
      ).rejects.toMatchObject({ code: 'TIMEOUT' })
      await waitFor(() => seen[key], `the handler's read ended (${key})`)
      expect(seen[key]).toBe('STREAM_ABORTED')
    }
  })

  it('an unread stream sent in two calls fails the second with STREAM_IN_USE (routed and local)', async () => {
    const { caller } = await pair((host) => {
      host.registerFunction(fid('ignore'), async (_input: unknown) => 'ignored')
    })
    caller.registerFunction(fid('local-ignore'), async (_input: unknown) => 'ignored')
    await localDispatchReady(caller, fid('local-ignore'))
    const kinds: Record<string, () => Promise<InputStream | OutputStream>> = {
      'InputStream.from': async () => InputStream.from('unread'),
      'pipe().input': async () => (await caller.pipe()).input,
      'pipe().output': async () => (await caller.pipe()).output,
    }
    for (const function_id of [fid('ignore'), fid('local-ignore')]) {
      for (const [kind, make] of Object.entries(kinds)) {
        const stream = await make()
        await expect(caller.call({ function_id, payload: { stream } }), kind).resolves.toBe('ignored')
        await expect(caller.call({ function_id, payload: { stream } }), kind).rejects.toMatchObject({
          code: 'STREAM_IN_USE',
          message: expect.stringMatching(/already sent/),
        })
      }
      // A stream sent away is no longer the caller's to read.
      const sent = InputStream.from('mine?')
      await caller.call({ function_id, payload: { sent } })
      await expect(sent.text()).rejects.toMatchObject({ code: 'STREAM_IN_USE' })
    }
  })

  it('a sink whose writes never settle does not hold a call past its timeout, and the held write fails (routed and local)', async () => {
    const writeErrors: Record<string, unknown> = {}
    const writer =
      (key: string) =>
      async ({ out }: { out: OutputStream }) => {
        try {
          for (let i = 0; i < 4; i++) await out.write('x')
          return 'wrote'
        } catch (error) {
          writeErrors[key] = (error as { code?: string }).code
          return 'cut'
        }
      }
    const { caller } = await pair((host) => {
      host.registerFunction(fid('write-stuck'), writer('routed'))
    })
    caller.registerFunction(fid('local-write-stuck'), writer('local'))
    await localDispatchReady(caller, fid('local-write-stuck'))
    for (const function_id of [fid('write-stuck'), fid('local-write-stuck')]) {
      const stuck = new WritableStream<Uint8Array>(
        { write: () => new Promise<void>(() => undefined) },
        { highWaterMark: 1 },
      )
      const started = performance.now()
      const error = await within(caller.call({ function_id, payload: { out: stuck }, timeoutMs: 800 }), 3000).catch(
        (e: unknown) => e,
      )
      expect(error).toMatchObject({ code: 'TIMEOUT' })
      expect(performance.now() - started).toBeLessThan(1500)
    }
    // In-process, the handler wrote into the stuck sink itself: the abort at the timeout cut its write.
    await waitFor(() => writeErrors.local, 'the held write failed')
    expect(writeErrors.local).toBe('STREAM_CLOSED')
  })
})
