/**
 * Call-native streams: {@link InputStream} (read side) and {@link OutputStream}
 * (write side), values that travel in call payloads and results. On the wire
 * each is a channel ref (`{ channel_id, access_key, direction }`); the SDK
 * creates the pipes, swaps in the refs and pumps the data (see `iii.ts`).
 *
 * The Node SDK's model on the browser's own types: bytes are `Uint8Array`s,
 * the native bridges are Web streams, and the sockets are native `WebSocket`s.
 */
import { type ChannelItem, ChannelReader, ChannelWriter, streamClosedError, streamInUseError } from './channels'
import { StreamAbortedError } from './errors'
import type { StreamChannelRef } from './iii-types'
import { randomUUID } from './utils'

/**
 * What an observer's target sees in place of a stream that the observed call
 * carried in its `input` or `output`: that a stream was there, its direction
 * and its channel id, without the access key (a stream has one reader, and it
 * is the observed function). The engine still sends the key on the wire; the
 * SDK drops it.
 */
export type StreamInfo = { stream: true; direction: 'read' | 'write'; id: string }

/** What {@link InputStream.from} reads from. */
export type InputStreamSource =
  | InputStream
  | ReadableStream<Uint8Array>
  | Blob
  | Uint8Array
  | ArrayBuffer
  | string
  | AsyncIterable<Uint8Array | string>
  | Iterable<Uint8Array | string>

/** What {@link OutputStream.to} writes into. */
export type OutputStreamSink = WritableStream<Uint8Array>

/** One frame of {@link InputStream.frames}: bytes, or a text message. */
export type StreamFrame = { type: 'binary'; value: Uint8Array } | { type: 'text'; value: string }

/** The engine a stream's socket connects to. */
export type StreamBinding = { wsBase: string }

/** Where the frames of an input stream come from. */
type FrameSource = { next(): Promise<ChannelItem | null>; cancel(): Promise<void> }

/** Where the frames of an output stream go. */
type FrameSink = {
  write(chunk: Uint8Array): Promise<void>
  send(text: string): Promise<void>
  close(): Promise<void>
  abort(reason: string): Promise<void>
}

type InputState = {
  ref?: StreamChannelRef
  binding?: StreamBinding
  source?: FrameSource
  /** A reader took it (R6: one reader). */
  claimed: boolean
  /** A frame was asked for: the socket (or the local source) is open. */
  started: boolean
  /** Its end, abort or cancel was reached. */
  finished: boolean
  failure?: unknown
  /** Sent to another function by its ref: this worker can no longer read it. */
  transferred: boolean
  id?: string
  scope?: StreamScope
}

type OutputState = {
  ref?: StreamChannelRef
  binding?: StreamBinding
  sink?: FrameSink
  /** Built by {@link OutputStream.to}: a byte sink in this page. */
  local: boolean
  used: boolean
  kept: boolean
  transferred: boolean
  /** How it was ended (`close()` or `abort()`), once one was asked for. */
  ended?: 'closed' | 'aborted'
  end?: Promise<void>
  failure?: unknown
  chain: Promise<unknown>
  /** Settles when the stream finished: resolves after a clean close, rejects after an abort. */
  done: Promise<void>
  settleDone: (error?: unknown) => void
  id?: string
  scope?: StreamScope
}

const inputs = new WeakMap<InputStream, InputState>()
const outputs = new WeakMap<OutputStream, OutputState>()

function inputState(s: InputStream): InputState {
  return inputs.get(s) as InputState
}

function outputState(s: OutputStream): OutputState {
  return outputs.get(s) as OutputState
}

const noop = () => undefined
const ENCODER = new TextEncoder()

/** `p`, marked handled: a caller that does not await an operation does not get an unhandled rejection when it fails. */
function handled<T>(p: Promise<T>): Promise<T> {
  p.catch(noop)
  return p
}

/** The text of a failure: an `Error`'s message, an engine error body's message, a string, or `String()`. */
export function errorText(error: unknown): string {
  // An abort passed on is the same abort: its reason, not its message, or each hop would prefix it again.
  if (error instanceof StreamAbortedError) return error.reason
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  const message = (error as { message?: unknown } | null | undefined)?.message
  if (typeof message === 'string') return message
  return String(error ?? 'aborted')
}

function toBytes(chunk: unknown, what: string): Uint8Array {
  if (chunk instanceof Uint8Array) return chunk
  if (typeof chunk === 'string') return ENCODER.encode(chunk)
  if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk)
  throw new TypeError(`${what} must be a Uint8Array or a string, got ${chunk === null ? 'null' : typeof chunk}`)
}

function concatBytes(chunks: Uint8Array[]): Uint8Array {
  if (chunks.length === 1) return chunks[0]
  let size = 0
  for (const chunk of chunks) size += chunk.byteLength
  const out = new Uint8Array(size)
  let offset = 0
  for (const chunk of chunks) {
    out.set(chunk, offset)
    offset += chunk.byteLength
  }
  return out
}

function transferredError(): Error {
  return streamInUseError('the stream was sent to another function and can no longer be used here')
}

/** Whether `value` is a `ReadableStream`, also one made in another realm (an iframe). */
function isReadableStream(value: object): value is ReadableStream<Uint8Array> {
  return (
    (typeof ReadableStream !== 'undefined' && value instanceof ReadableStream) ||
    Object.prototype.toString.call(value) === '[object ReadableStream]'
  )
}

/** Whether `value` is a `WritableStream`, also one made in another realm. */
function isWritableStream(value: object): value is WritableStream<Uint8Array> {
  return (
    (typeof WritableStream !== 'undefined' && value instanceof WritableStream) ||
    Object.prototype.toString.call(value) === '[object WritableStream]'
  )
}

/** Whether `value` is a `Blob` or a `File`, also one made in another realm. */
function isBlob(value: object): value is Blob {
  if (typeof Blob !== 'undefined' && value instanceof Blob) return true
  const tag = Object.prototype.toString.call(value)
  return (tag === '[object Blob]' || tag === '[object File]') && typeof (value as Blob).stream === 'function'
}

// ── InputStream ────────────────────────────────────────────────────────────────

/** Builds an InputStream from its state (the constructor is private). */
const makeInput = (state: InputState): InputStream =>
  new (InputStream as unknown as new (state: InputState) => InputStream)(state)

/** Builds an OutputStream from its state (the constructor is private). */
const makeOutput = (state: Omit<OutputState, 'done' | 'settleDone' | 'chain'>): OutputStream =>
  new (OutputStream as unknown as new (state: Omit<OutputState, 'done' | 'settleDone' | 'chain'>) => OutputStream)(
    state,
  )

/**
 * The read side of a stream. A field of a handler's input or of a call's
 * result arrives as one when the sender put a source there (a Web
 * `ReadableStream`, a `Blob` or `File`, an async generator, an
 * `InputStream`, or a Node `Readable` from a Node worker). Build one yourself
 * with {@link InputStream.from}.
 *
 * A stream has one reader: read it once, with one of `for await`,
 * {@link bytes}, {@link text}, {@link lines}, {@link messages}, {@link frames}
 * or {@link toWebStream}. Call {@link tee} first for two. Pass it on unread in
 * a payload or a result and its ref travels instead: no bytes go through this
 * worker. Inside a handler, a stream the handler started reading and did not
 * finish is cancelled when the handler returns.
 *
 * A stream carries bytes (binary frames) and, separately, JSON messages (text
 * frames): byte reads skip the messages, {@link messages} skips the bytes, and
 * {@link frames} yields both in order. An abort surfaces as a
 * {@link StreamAbortedError} from the read, after the data sent before it.
 *
 * @example
 * ```typescript
 * iii.registerFunction('files::process', async ({ body }: { body: InputStream }) => {
 *   let bytes = 0
 *   for await (const chunk of body) bytes += chunk.byteLength
 *   return { bytes }
 * })
 * ```
 */
export class InputStream implements AsyncIterable<Uint8Array> {
  private constructor(state: InputState) {
    inputs.set(this, state)
  }

  /**
   * An InputStream that reads `source`: a Web `ReadableStream`, a `Blob` or
   * `File`, a `Uint8Array`, an `ArrayBuffer`, a string (UTF-8), or a sync or
   * async iterable of `Uint8Array` and strings. Nothing is read until the
   * stream is.
   */
  static from(source: InputStreamSource): InputStream {
    if (source instanceof InputStream) return source
    return makeInput(localInputState(frameSourceOf(source)))
  }

  /** The stream's bytes, chunk by chunk (messages are skipped). Breaking out of the loop cancels the stream. */
  [Symbol.asyncIterator](): AsyncIterator<Uint8Array> {
    return binaryChunks(claim(this))
  }

  /** Reads the whole stream; resolves to its bytes (messages are skipped). */
  async bytes(): Promise<Uint8Array> {
    const st = claim(this)
    const chunks: Uint8Array[] = []
    for await (const chunk of binaryChunks(st)) chunks.push(chunk)
    return concatBytes(chunks)
  }

  /** Reads the whole stream as UTF-8 text. */
  async text(): Promise<string> {
    return new TextDecoder('utf-8').decode(await this.bytes())
  }

  /**
   * The stream's text, line by line: UTF-8, without the `\n` (or `\r\n`); the
   * last line comes without a newline too.
   */
  lines(): AsyncIterableIterator<string> {
    return textLines(claim(this))
  }

  /**
   * The stream's JSON messages (text frames), each parsed; one that is not
   * JSON comes as its raw string. Bytes are skipped.
   */
  messages<T = unknown>(): AsyncIterableIterator<T> {
    return jsonMessages(claim(this)) as AsyncIterableIterator<T>
  }

  /** Bytes and messages in the order they were sent. */
  frames(): AsyncIterableIterator<StreamFrame> {
    return allFrames(claim(this))
  }

  /**
   * Stops reading. The writer is told the reader left (the engine closes its
   * end). A stream that never opened is opened for that. Idempotent.
   */
  cancel(): Promise<void> {
    const st = inputState(this)
    if (st.transferred) return Promise.resolve()
    return handled(cancelInput(st, true))
  }

  /**
   * Two streams that each read everything this one carries, in this page.
   * Call it before reading. Reading is held to the slower copy: one that is
   * not read stops the other after 16 frames, so cancel a copy you do not need.
   */
  tee(): [InputStream, InputStream] {
    return teeInput(claim(this))
  }

  /** A Web `ReadableStream` of the stream's bytes. Cancelling it cancels the stream. */
  toWebStream(): ReadableStream<Uint8Array> {
    const chunks = binaryChunks(claim(this))
    return new ReadableStream<Uint8Array>(
      {
        async pull(controller) {
          const next = await chunks.next()
          if (next.done) controller.close()
          else controller.enqueue(next.value)
        },
        async cancel() {
          await chunks.return(undefined)
        },
      },
      { highWaterMark: 0 },
    )
  }

  /** The {@link StreamInfo} of the stream: what `JSON.stringify` writes for it (never its access key). */
  toJSON(): StreamInfo {
    return { stream: true, direction: 'read', id: streamId(inputState(this)) }
  }
}

function localInputState(source: FrameSource): InputState {
  return { source, claimed: false, started: false, finished: false, transferred: false }
}

function streamId(st: { ref?: StreamChannelRef; id?: string }): string {
  if (st.ref) return st.ref.channel_id
  st.id ??= `local:${randomUUID()}`
  return st.id
}

function oneReaderError(): Error {
  return streamInUseError(
    'the InputStream is already being read: a stream has one reader (call tee() before reading for two)',
  )
}

function claim(s: InputStream): InputState {
  const st = inputState(s)
  if (st.transferred) throw transferredError()
  if (st.claimed) throw oneReaderError()
  st.claimed = true
  return st
}

async function pullFrame(st: InputState): Promise<ChannelItem | null> {
  if (st.finished) {
    if (st.failure !== undefined) throw st.failure
    return null
  }
  if (st.transferred) throw transferredError()
  st.started = true
  if (!st.source) {
    const reader = new ChannelReader(st.binding?.wsBase ?? '', st.ref as StreamChannelRef)
    st.source = { next: () => reader.next(), cancel: () => reader.cancel() }
  }
  try {
    const item = await st.source.next()
    if (item === null) st.finished = true
    return item
  } catch (error) {
    // A source that throws aborts the stream, in this page as across the engine.
    const failure =
      error instanceof StreamAbortedError ? error : new StreamAbortedError(errorText(error), { cause: error })
    st.finished = true
    st.failure = failure
    throw failure
  }
}

async function cancelInput(st: InputState, attach: boolean): Promise<void> {
  if (st.finished) return
  st.finished = true
  if (st.source) {
    await st.source.cancel()
  } else if (attach && st.ref) {
    // Never opened: attach and leave, so the writer learns the reader is gone.
    await new ChannelReader(st.binding?.wsBase ?? '', st.ref).cancel(true)
  }
}

async function* binaryChunks(st: InputState): AsyncGenerator<Uint8Array, void, undefined> {
  try {
    for (;;) {
      const item = await pullFrame(st)
      if (item === null) return
      if (item.type === 'binary' && item.value.byteLength > 0) yield item.value
    }
  } finally {
    // A loop left early (break, return, a throw in its body) cancels the stream.
    if (!st.finished) await cancelInput(st, false)
  }
}

async function* allFrames(st: InputState): AsyncGenerator<StreamFrame, void, undefined> {
  try {
    for (;;) {
      const item = await pullFrame(st)
      if (item === null) return
      yield item
    }
  } finally {
    if (!st.finished) await cancelInput(st, false)
  }
}

async function* jsonMessages(st: InputState): AsyncGenerator<unknown, void, undefined> {
  try {
    for (;;) {
      const item = await pullFrame(st)
      if (item === null) return
      if (item.type !== 'text') continue
      let message: unknown
      try {
        message = JSON.parse(item.value)
      } catch {
        message = item.value
      }
      yield message
    }
  } finally {
    if (!st.finished) await cancelInput(st, false)
  }
}

function withoutCr(line: string): string {
  return line.endsWith('\r') ? line.slice(0, -1) : line
}

async function* textLines(st: InputState): AsyncGenerator<string, void, undefined> {
  const decoder = new TextDecoder('utf-8')
  let pending = ''
  try {
    for (;;) {
      const item = await pullFrame(st)
      if (item === null) break
      if (item.type !== 'binary') continue
      pending += decoder.decode(item.value, { stream: true })
      let start = 0
      for (;;) {
        const newline = pending.indexOf('\n', start)
        if (newline < 0) break
        yield withoutCr(pending.slice(start, newline))
        start = newline + 1
      }
      pending = pending.slice(start)
    }
    pending += decoder.decode()
    if (pending.length > 0) yield withoutCr(pending)
  } finally {
    if (!st.finished) await cancelInput(st, false)
  }
}

/** Frames a branch of a tee may hold before it holds the other branch back. */
const TEE_HIGH_WATER_FRAMES = 16

function teeInput(st: InputState): [InputStream, InputStream] {
  const queues: ChannelItem[][] = [[], []]
  const active = [true, true]
  let end: { error?: unknown } | undefined
  let pulling: Promise<void> | undefined
  let waiters: Array<() => void> = []
  const notify = () => {
    const ready = waiters
    waiters = []
    for (const resolve of ready) resolve()
  }
  const changed = () => new Promise<void>((resolve) => waiters.push(resolve))
  const pullOnce = (): Promise<void> => {
    pulling ??= (async () => {
      try {
        const item = await pullFrame(st)
        if (item === null) end = {}
        else for (let i = 0; i < 2; i++) if (active[i]) queues[i].push(item)
      } catch (error) {
        end = { error }
      } finally {
        pulling = undefined
        notify()
      }
    })()
    return pulling
  }
  const branch = (i: number): InputStream =>
    makeInput(
      localInputState({
        next: async () => {
          for (;;) {
            const item = queues[i].shift()
            if (item) {
              notify()
              return item
            }
            if (end) {
              if (end.error !== undefined) throw end.error
              return null
            }
            if (active[1 - i] && queues[1 - i].length >= TEE_HIGH_WATER_FRAMES) await changed()
            else await pullOnce()
          }
        },
        cancel: async () => {
          active[i] = false
          queues[i].length = 0
          notify()
          if (!active[0] && !active[1] && !end) await cancelInput(st, false)
        },
      }),
    )
  return [branch(0), branch(1)]
}

function frameSourceOf(source: InputStreamSource): FrameSource {
  if (typeof source === 'string' || source instanceof Uint8Array || source instanceof ArrayBuffer) {
    const bytes = toBytes(source, 'the source')
    return iteratorSource(() => [bytes][Symbol.iterator]())
  }
  if (source !== null && typeof source === 'object') {
    if (isReadableStream(source)) return webSource(() => source)
    if (isBlob(source)) return webSource(() => source.stream() as unknown as ReadableStream<Uint8Array>)
    const asyncIter = (source as AsyncIterable<unknown>)[Symbol.asyncIterator]
    if (typeof asyncIter === 'function') return iteratorSource(() => asyncIter.call(source))
    const syncIter = (source as Iterable<unknown>)[Symbol.iterator]
    if (typeof syncIter === 'function') return iteratorSource(() => syncIter.call(source))
  }
  throw new TypeError(
    'InputStream.from takes a ReadableStream, a Blob, a Uint8Array, an ArrayBuffer, a string, or an iterable of Uint8Array and strings',
  )
}

function iteratorSource(open: () => Iterator<unknown> | AsyncIterator<unknown>): FrameSource {
  let it: Iterator<unknown> | AsyncIterator<unknown> | undefined
  return {
    next: async () => {
      it ??= open()
      for (;;) {
        const step = await it.next()
        if (step.done) return null
        const bytes = toBytes(step.value, 'a chunk of a stream source')
        if (bytes.byteLength > 0) return { type: 'binary', value: bytes }
      }
    },
    cancel: async () => {
      // Closes the source, read or not: `return()` ends a generator (running its finally).
      it ??= open()
      await it.return?.()
    },
  }
}

function webSource(open: () => ReadableStream<Uint8Array>): FrameSource {
  let reader: ReadableStreamDefaultReader<Uint8Array> | undefined
  return {
    next: async () => {
      reader ??= open().getReader()
      for (;;) {
        const { done, value } = await reader.read()
        if (done) return null
        const bytes = toBytes(value, 'a chunk of a ReadableStream')
        if (bytes.byteLength > 0) return { type: 'binary', value: bytes }
      }
    },
    cancel: async () => {
      if (reader) await reader.cancel()
      else await open().cancel()
    },
  }
}

// ── OutputStream ───────────────────────────────────────────────────────────────

/**
 * The write side of a stream. A field of a handler's input arrives as one when
 * the caller put a sink there (a Web `WritableStream`, an `OutputStream`, or a
 * Node `Writable` from a Node worker); the call resolves once what the handler
 * wrote reached the sink. Build one yourself with {@link OutputStream.to}.
 *
 * Inside a handler, a stream the handler wrote to (or bridged) is closed when
 * the handler returns and aborted when it throws, unless it was closed,
 * aborted, sent on, or kept open with {@link keepOpen}. One the handler never
 * touched is closed when it returns, whatever it returns. In an observer
 * activation (an HTTP route target), it is closed only when the handler returns
 * `undefined` or `null`, and left alone otherwise: the HTTP worker reads an
 * untouched response and a value as a buffered response.
 *
 * @example
 * ```typescript
 * iii.registerFunction('reports::render-pdf', async ({ out }: { out: OutputStream }) => {
 *   for await (const page of renderPages()) await out.write(page)
 *   // returning closes `out`; throwing aborts it, so the caller sees an error, not a short file
 * })
 * ```
 */
export class OutputStream {
  private constructor(state: Omit<OutputState, 'done' | 'settleDone' | 'chain'>) {
    let settleDone: (error?: unknown) => void = noop
    const done = handled(
      new Promise<void>((resolve, reject) => {
        settleDone = (error) => (error === undefined ? resolve() : reject(error))
      }),
    )
    outputs.set(this, { ...state, chain: Promise.resolve(), done, settleDone })
  }

  /**
   * An OutputStream that writes into `sink`, a Web `WritableStream`: bytes
   * only (messages sent to it are dropped). Closing it closes the sink;
   * aborting it aborts the sink with a {@link StreamAbortedError}.
   */
  static to(sink: OutputStreamSink): OutputStream {
    return makeOutput({ sink: frameSinkOf(sink), local: true, used: false, kept: false, transferred: false })
  }

  /** Writes bytes (a string is written as UTF-8). Resolves when the chunk was accepted. */
  write(chunk: Uint8Array | string): Promise<void> {
    const st = outputState(this)
    const refused = refusal(st)
    if (refused) return handled(Promise.reject(refused))
    st.used = true
    let bytes: Uint8Array
    try {
      bytes = toBytes(chunk, 'the chunk')
    } catch (error) {
      return handled(Promise.reject(error))
    }
    return handled(enqueue(st, (sink) => sink.write(bytes)))
  }

  /** Sends one JSON message (`JSON.stringify(message)`) in a text frame. Resolves when it was accepted. */
  send(message: unknown): Promise<void> {
    const st = outputState(this)
    const refused = refusal(st)
    if (refused) return handled(Promise.reject(refused))
    let text: string | undefined
    try {
      // A BigInt or a cycle throws here: a rejected promise, as for any other failed send.
      text = JSON.stringify(message)
    } catch (error) {
      return handled(Promise.reject(error))
    }
    if (text === undefined) return handled(Promise.reject(new TypeError('the message is not JSON-serializable')))
    st.used = true
    return handled(enqueue(st, (sink) => sink.send(text)))
  }

  /**
   * Ends the stream: the reader sees its end after everything written before.
   * A stream that never opened is opened, so the reader sees an empty stream.
   * Idempotent; writing afterwards fails.
   */
  close(): Promise<void> {
    const st = outputState(this)
    if (st.transferred) return Promise.resolve()
    if (st.end) return st.end
    st.ended = 'closed'
    st.end = handled(
      enqueue(st, (sink) => sink.close()).then(
        () => st.settleDone(),
        (error) => {
          st.settleDone(error)
          throw error
        },
      ),
    )
    return st.end
  }

  /**
   * Fails the stream: the reader gets a {@link StreamAbortedError} instead of
   * an end, after the data the socket already took. A stream that never opened
   * is opened first, so the reader always learns of it. Idempotent.
   */
  abort(reason?: string | Error): Promise<void> {
    const st = outputState(this)
    if (st.transferred) return Promise.resolve()
    if (st.end) return st.end
    const text = reason === undefined ? 'aborted' : errorText(reason)
    st.ended = 'aborted'
    // Not queued behind writes in flight: an abort cuts the stream now.
    st.end = handled(
      sinkOf(st)
        .abort(text)
        .then(() => st.settleDone(new StreamAbortedError(text))),
    )
    return st.end
  }

  /**
   * Keeps the stream open after the handler that received it returns: the
   * handler's code then closes or aborts it itself. Returns the stream.
   */
  keepOpen(): this {
    outputState(this).kept = true
    return this
  }

  /** A Web `WritableStream` into the stream. Closing it closes the stream; aborting it aborts the stream. */
  toWebStream(): WritableStream<Uint8Array> {
    outputState(this).used = true
    return new WritableStream<Uint8Array>({
      write: (chunk) => this.write(chunk),
      close: () => this.close(),
      abort: (reason) => this.abort(reason === undefined ? undefined : errorText(reason)),
    })
  }

  /** The {@link StreamInfo} of the stream: what `JSON.stringify` writes for it (never its access key). */
  toJSON(): StreamInfo {
    return { stream: true, direction: 'write', id: streamId(outputState(this)) }
  }
}

function refusal(st: OutputState): Error | undefined {
  if (st.transferred) return transferredError()
  if (st.ended) return streamClosedError(`the stream was already ${st.ended === 'closed' ? 'closed' : 'aborted'}`)
  return undefined
}

function sinkOf(st: OutputState): FrameSink {
  if (st.sink) return st.sink
  const writer = new ChannelWriter(st.binding?.wsBase ?? '', st.ref as StreamChannelRef)
  st.sink = {
    write: (chunk) => writer.writeBinary(chunk),
    send: (text) => writer.writeText(text),
    close: () => writer.close(),
    abort: (reason) => writer.abort(reason),
  }
  return st.sink
}

/** Runs `op` after the operations before it, so frames keep their order. */
function enqueue(st: OutputState, op: (sink: FrameSink) => Promise<void>): Promise<void> {
  const run = st.chain.then(() => {
    if (st.failure !== undefined) throw st.failure
    return op(sinkOf(st))
  })
  st.chain = run.catch((error) => {
    st.failure ??= error
  })
  return run
}

function frameSinkOf(sink: OutputStreamSink): FrameSink {
  if (sink !== null && typeof sink === 'object' && isWritableStream(sink)) return webSink(sink)
  throw new TypeError('OutputStream.to takes a WritableStream')
}

function webSink(ws: WritableStream<Uint8Array>): FrameSink {
  let writer: WritableStreamDefaultWriter<Uint8Array> | undefined
  const open = () => {
    writer ??= ws.getWriter()
    return writer
  }
  return {
    write: async (chunk) => {
      const w = open()
      await w.ready
      await w.write(chunk)
    },
    // A byte sink: messages have nowhere to go.
    send: async () => undefined,
    close: () => open().close(),
    abort: (reason) => open().abort(new StreamAbortedError(reason)),
  }
}

// ── Internal API for the call path ─────────────────────────────────────────────

/** An InputStream that reads the channel end `ref` (a read ref). */
export function inputFromRef(ref: StreamChannelRef, binding: StreamBinding): InputStream {
  return makeInput({ ref, binding, claimed: false, started: false, finished: false, transferred: false })
}

/** An OutputStream that writes the channel end `ref` (a write ref). */
export function outputFromRef(ref: StreamChannelRef, binding: StreamBinding): OutputStream {
  return makeOutput({ ref, binding, local: false, used: false, kept: false, transferred: false })
}

/**
 * The ref `s` travels as when it is passed on as it is (R5): an InputStream
 * never read, an OutputStream never used, both still this worker's. `undefined`
 * when the stream has to be pumped through a new pipe instead.
 */
export function forwardableRef(s: InputStream | OutputStream): StreamChannelRef | undefined {
  if (s instanceof InputStream) {
    const st = inputState(s)
    return st.ref && !st.started && !st.claimed && !st.finished && !st.transferred ? st.ref : undefined
  }
  const st = outputState(s)
  return st.ref && !st.used && !st.ended && !st.transferred ? st.ref : undefined
}

/** `s` went to another function: it leaves its invocation's guard, and this worker can no longer use it. */
export function markTransferred(s: InputStream | OutputStream): void {
  const st = s instanceof InputStream ? inputState(s) : outputState(s)
  st.transferred = true
  detach(s)
}

/** `s` leaves the guard of the invocation that received it (a pump or a caller in this page owns it now). */
export function detach(s: InputStream | OutputStream): void {
  const st = s instanceof InputStream ? inputState(s) : outputState(s)
  st.scope?.members.delete(s)
  st.scope = undefined
}

/** Whether `s` was already sent away by its ref. */
export function isTransferred(s: InputStream | OutputStream): boolean {
  return (s instanceof InputStream ? inputState(s) : outputState(s)).transferred
}

/** An OutputStream built by {@link OutputStream.to}: a sink in this page. */
export function isLocalSink(s: OutputStream): boolean {
  return outputState(s).local
}

/** Settles once `s` finished: resolves after its clean close, rejects with a StreamAbortedError after an abort. */
export function outputDone(s: OutputStream): Promise<void> {
  return outputState(s).done
}

/** Whether `s` was closed or aborted. */
export function outputEnded(s: OutputStream): boolean {
  return outputState(s).ended !== undefined
}

/** Whether a read of `s` began. */
export function inputStarted(s: InputStream): boolean {
  return inputState(s).started
}

/** Cancels `s` without opening a socket that never opened. */
export function releaseInput(s: InputStream): Promise<void> {
  return handled(cancelInput(inputState(s), false))
}

/** Why {@link handOn} would refuse `s`, checked with no effect: a stream read to its end (R6). */
export function handOnRefusal(s: InputStream): Error | undefined {
  const st = inputState(s)
  return st.claimed && st.finished ? oneReaderError() : undefined
}

/**
 * What `s` is passed on as when it goes to another function without its ref
 * (in-process, or pumped into a pipe). Never read: `s` itself. A read began
 * and did not finish: a new InputStream over the rest, which leaves the guard
 * of the invocation that holds `s`. Read to its end: the one-reader error, as
 * reading it again would throw.
 */
export function handOn(s: InputStream): InputStream {
  const refused = handOnRefusal(s)
  if (refused) throw refused
  const st = inputState(s)
  if (!st.claimed) return s
  detach(s)
  return makeInput(localInputState({ next: () => pullFrame(st), cancel: () => cancelInput(st, false) }))
}

/** The {@link StreamInfo} of a channel ref. */
export function streamInfoOf(ref: StreamChannelRef): StreamInfo {
  return { stream: true, direction: ref.direction, id: ref.channel_id }
}

/**
 * Copies `input` into `output`, frame by frame, with backpressure: bytes are
 * written, messages sent as they came. Ends `output` when `input` ends; aborts
 * it when `input` fails (and rejects with that failure); cancels `input` when
 * `output` fails (and rejects with that failure).
 */
export async function pumpStream(input: InputStream, output: OutputStream): Promise<void> {
  const ist = inputState(input)
  const ost = outputState(output)
  ist.claimed = true
  for (;;) {
    let item: ChannelItem | null
    try {
      item = await pullFrame(ist)
    } catch (error) {
      await output.abort(errorText(error)).catch(noop)
      throw error instanceof StreamAbortedError ? error : new StreamAbortedError(errorText(error), { cause: error })
    }
    if (item === null) break
    try {
      if (item.type === 'binary') {
        const bytes = item.value
        await enqueue(ost, (sink) => sink.write(bytes))
      } else {
        const text = item.value
        await enqueue(ost, (sink) => sink.send(text))
      }
    } catch (error) {
      await cancelInput(ist, false).catch(noop)
      throw error
    }
  }
  await output.close()
}

/**
 * The streams one invocation received in its input, and what happens to them
 * when its handler settles (R2, R3): see {@link StreamScope.settle}.
 */
export class StreamScope {
  readonly members = new Set<InputStream | OutputStream>()

  /** `s` is this invocation's now (it leaves the guard of the one that had it). */
  adopt(s: InputStream | OutputStream): void {
    const st = s instanceof InputStream ? inputState(s) : outputState(s)
    if (st.scope === this) return
    st.scope?.members.delete(s)
    st.scope = this
    this.members.add(s)
  }

  /**
   * The handler settled. An OutputStream it used (wrote, sent, bridged) and did
   * not end is closed on success and aborted on failure. One it never used is
   * closed on success of a plain call, whatever the result, so the caller's
   * sink ends; for an observer activation (`activation`, e.g. an HTTP route
   * target) only when the result is `undefined`/`null`, and left alone
   * otherwise (the HTTP worker reads an untouched response and a value as a
   * buffered response). On failure it is left alone. An InputStream the
   * handler started reading and did not finish is cancelled; one never read is
   * left alone. Streams sent on or kept open are skipped. Starts the closes and
   * returns.
   *
   * `local` is for a call run in-process: a source of this page (a
   * `ReadableStream`, a generator) the handler never read nor passed on is
   * also cancelled, since nothing else would ever release it.
   */
  settle(
    outcome: { ok: true; value: unknown } | { ok: false; error: unknown },
    mode: { activation?: boolean; local?: boolean } = {},
  ): void {
    const closeUntouched = outcome.ok && (!mode.activation || outcome.value === undefined || outcome.value === null)
    for (const s of this.members) {
      if (s instanceof OutputStream) {
        const st = outputState(s)
        st.scope = undefined
        if (st.ended || st.kept || st.transferred) continue
        if (st.used) {
          if (outcome.ok) void s.close()
          else void s.abort(errorText(outcome.error))
        } else if (closeUntouched) {
          void s.close()
        }
      } else {
        const st = inputState(s)
        st.scope = undefined
        if (st.transferred || st.finished) continue
        if (st.started || (mode.local && !st.claimed && st.ref === undefined)) void releaseInput(s)
      }
    }
    this.members.clear()
  }
}

/** What the outbound walk does with a value: a stream type, a native source or a native sink. */
export function streamKind(value: unknown): 'input' | 'output' | 'source' | 'sink' | undefined {
  if (value === null || typeof value !== 'object') return undefined
  if (value instanceof InputStream) return 'input'
  if (value instanceof OutputStream) return 'output'
  if (isReadableStream(value)) return 'source'
  if (isBlob(value)) return 'source'
  if (Object.prototype.toString.call(value) === '[object AsyncGenerator]') return 'source'
  if (isWritableStream(value)) return 'sink'
  return undefined
}

/** The InputStream or OutputStream a detected value is used through. */
export function asStream(value: unknown): InputStream | OutputStream {
  const kind = streamKind(value)
  if (kind === 'input' || kind === 'output') return value as InputStream | OutputStream
  if (kind === 'sink') return OutputStream.to(value as OutputStreamSink)
  return InputStream.from(value as InputStreamSource)
}
