/**
 * The two WebSocket ends of an engine channel (`/ws/channels/<id>`), the
 * transport under {@link InputStream} and {@link OutputStream}, on the
 * browser's native `WebSocket`. Internal: the public API is the stream types;
 * the low-level wire names are in `iii-browser-sdk/protocol`.
 *
 * A browser socket sends no custom headers, so a channel authenticates the way
 * the worker's own connection does: through the query of the engine URL, which
 * the channel URL keeps, or cookies.
 */
import { InvocationError, StreamAbortedError } from './errors'
import type { StreamChannelRef } from './iii-types'

/**
 * Direction of a streaming channel endpoint. Mirrors the Rust SDK's
 * `ChannelDirection` enum and matches the literal values used by
 * {@link StreamChannelRef.direction}.
 */
export const ChannelDirection = {
  Read: 'read',
  Write: 'write',
} as const
export type ChannelDirection = (typeof ChannelDirection)[keyof typeof ChannelDirection]

/**
 * One frame of a channel: a binary frame carries bytes, a text frame a
 * message. Mirrors the Rust SDK's `ChannelItem` enum (`Text` / `Binary`);
 * `InputStream.frames()` yields these.
 */
export type ChannelItem = { type: 'text'; value: string } | { type: 'binary'; value: Uint8Array }

export const ChannelItem = {
  /** Construct a text channel item. */
  Text(value: string): ChannelItem {
    return { type: 'text', value }
  },
  /** Construct a binary channel item. */
  Binary(value: Uint8Array): ChannelItem {
    return { type: 'binary', value }
  },
} as const

/** Binary data goes out in frames of at most this many bytes. */
const FRAME_SIZE = 64 * 1024
/**
 * Delay between the last data frame and the close frame, so the data leaves
 * first: without it the close frame can reach the engine before the data,
 * which truncates the stream. Same as the Node SDK.
 */
const CLOSE_DELAY_MS = 10
/**
 * The close code an abort sends. The engine relays any code but 1000 to the
 * reader as an abort; a browser's `WebSocket.close()` accepts only 1000 and
 * 3000-4999 (the Node SDK sends 1011), so the abort takes 1011 in the
 * private-use range.
 */
export const ABORT_CLOSE_CODE = 4011
/** A WebSocket close reason holds at most 123 bytes. */
const MAX_CLOSE_REASON_BYTES = 123
/**
 * A write resolves once the socket holds at most this many bytes not yet sent
 * (`bufferedAmount`). A browser socket has no write callback: this is its
 * backpressure, so a loop of awaited writes follows the reader's pace.
 */
const WRITE_HIGH_WATER_BYTES = 1024 * 1024
/** How often a write that waits for the socket to drain looks again. */
const DRAIN_POLL_MS = 5

const CONNECTING = 0
const OPEN = 1

const ENCODER = new TextEncoder()
const DECODER = new TextDecoder('utf-8')

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

export function buildChannelUrl(
  engineWsBase: string,
  channelId: string,
  accessKey: string,
  direction: 'read' | 'write',
): string {
  // Keep a query already on the engine URL (e.g. `?token=...`); the channel's own come after it.
  const [base, ...rest] = engineWsBase.split('?')
  const query = [rest.join('?'), `key=${encodeURIComponent(accessKey)}`, `dir=${direction}`].filter(Boolean).join('&')
  return `${base.replace(/\/$/, '')}/ws/channels/${channelId}?${query}`
}

/** `reason` cut to what a close frame can carry, on a UTF-8 boundary. */
export function closeReason(reason: string): string {
  const bytes = ENCODER.encode(reason)
  if (bytes.length <= MAX_CLOSE_REASON_BYTES) return reason
  let cut = MAX_CLOSE_REASON_BYTES
  while (cut > 0 && (bytes[cut] & 0xc0) === 0x80) cut--
  return DECODER.decode(bytes.subarray(0, cut))
}

/** The error a write gets once its stream can no longer take data. */
export function streamClosedError(message: string, cause?: unknown): InvocationError {
  const error = new InvocationError({ code: 'STREAM_CLOSED', message })
  if (cause !== undefined) (error as { cause?: unknown }).cause = cause
  return error
}

/**
 * The error for a stream another reader or function already has: a second
 * read, the same stream twice in one payload, a stream passed on after it was
 * read to its end or sent away. `code` is `STREAM_IN_USE`.
 */
export function streamInUseError(message: string): InvocationError {
  return new InvocationError({ code: 'STREAM_IN_USE', message })
}

/** What a socket's `error` event says: `ws` (Node) gives a message; a browser gives none. */
function socketErrorText(event: Event): string {
  const message = (event as { message?: unknown }).message
  return typeof message === 'string' && message !== '' ? message : 'the connection failed'
}

/**
 * Write end of a channel. The socket opens on the first operation; what is
 * sent before it opens waits for it. Binary data goes out in 64 KiB frames,
 * text as one text frame, in order. A write resolves once the socket took the
 * frame and holds no more than {@link WRITE_HIGH_WATER_BYTES} unsent.
 * `close()` ends the stream cleanly (code 1000, after the socket drained and a
 * short flush delay); `abort()` closes with {@link ABORT_CLOSE_CODE}, which the
 * reader gets as an abort. Both open the socket first when it never opened, so
 * the reader always learns how the stream ended. Once the socket is gone (the
 * engine closes it when the reader leaves), every operation fails.
 */
export class ChannelWriter {
  private ws: WebSocket | null = null
  /** Settles `true` once the socket opened, `false` when it closed or failed first. */
  private opened: Promise<boolean> | null = null
  private closed: Promise<void> | null = null
  /** Why the writer can take no more data: set once, when it fails or ends. */
  private failure: Error | null = null
  /** Set once close() or abort() put (or will put) a close frame on the wire. */
  private ending: Promise<void> | null = null
  /** The sends in order: each waits for the one before it. */
  private tail: Promise<void> = Promise.resolve()
  private readonly url: string

  constructor(engineWsBase: string, ref: StreamChannelRef) {
    this.url = buildChannelUrl(engineWsBase, ref.channel_id, ref.access_key, 'write')
  }

  /** Sends `data` in binary frames; resolves once the socket took the last one. */
  writeBinary(data: Uint8Array): Promise<void> {
    if (data.byteLength <= FRAME_SIZE) return this.send(copyOf(data))
    const parts: Promise<void>[] = []
    for (let offset = 0; offset < data.byteLength; offset += FRAME_SIZE) {
      parts.push(this.send(copyOf(data.subarray(offset, Math.min(offset + FRAME_SIZE, data.byteLength)))))
    }
    return Promise.all(parts).then(() => undefined)
  }

  /** Sends one text frame; resolves once the socket took it. */
  writeText(text: string): Promise<void> {
    return this.send(text)
  }

  /** Ends the stream cleanly; resolves once the socket closed. */
  close(): Promise<void> {
    if (this.ending) return this.ending
    this.ending = this.finish(1000, 'stream_complete', true)
    return this.ending
  }

  /** Aborts the stream: the reader gets an error instead of an end. Resolves once the socket closed. */
  abort(reason: string): Promise<void> {
    if (this.ending) return this.ending
    this.ending = this.finish(ABORT_CLOSE_CODE, closeReason(reason || 'aborted'), false)
    return this.ending
  }

  private send(data: ArrayBuffer | string): Promise<void> {
    if (this.failure) return Promise.reject(this.failure)
    if (this.ending) return Promise.reject(streamClosedError('the stream was already closed'))
    this.ensureConnected()
    const run = this.tail.then(() => this.transmit(data))
    this.tail = run.catch(() => undefined)
    return run
  }

  private async transmit(data: ArrayBuffer | string): Promise<void> {
    await this.opened
    const ws = this.ws as WebSocket
    if (this.failure) throw this.failure
    if (ws.readyState !== OPEN) throw this.closedError()
    try {
      ws.send(data)
    } catch (error) {
      throw streamClosedError(`the stream's connection failed: ${String(error)}`, error)
    }
    // The browser's backpressure: wait while the socket holds too much it has not sent yet.
    while (ws.readyState === OPEN && ws.bufferedAmount > WRITE_HIGH_WATER_BYTES) await sleep(DRAIN_POLL_MS)
    // A socket that closed with this frame still queued did not send it.
    if (ws.readyState !== OPEN && ws.bufferedAmount > 0) throw this.failure ?? this.closedError()
  }

  private async finish(code: number, reason: string, drain: boolean): Promise<void> {
    if (this.failure) return
    this.ensureConnected()
    const ws = this.ws as WebSocket
    // A close sent while the handshake runs cancels it: the engine never sees this end attach.
    if (!(await this.opened)) return
    if (drain) {
      // A clean close goes after every write: they are queued before it, and the socket sends them first.
      await this.tail
      while (ws.readyState === OPEN && ws.bufferedAmount > 0) await sleep(DRAIN_POLL_MS)
      await sleep(CLOSE_DELAY_MS)
    }
    if (ws.readyState === OPEN) ws.close(code, reason)
    await this.closed
  }

  private ensureConnected(): void {
    if (this.ws) return
    const ws = new WebSocket(this.url)
    ws.binaryType = 'arraybuffer'
    this.ws = ws
    this.opened = new Promise<boolean>((resolve) => {
      ws.addEventListener('open', () => resolve(true))
      ws.addEventListener('close', () => resolve(false))
    })
    this.closed = new Promise<void>((resolve) => {
      ws.addEventListener('close', () => {
        this.fail(this.closedError())
        resolve()
      })
    })
    ws.addEventListener('error', (event) => {
      this.fail(streamClosedError(`the stream's connection failed: ${socketErrorText(event)}`))
    })
  }

  /** Our own close is the end we asked for; any other close means nobody reads what is sent now. */
  private closedError(): Error {
    return streamClosedError(this.ending ? 'the stream was already closed' : 'the reader closed the stream')
  }

  private fail(err: Error): void {
    this.failure ??= err
  }
}

/** A copy of `data` in an `ArrayBuffer` of its own: what the socket sends, whatever the caller does with `data` next. */
function copyOf(data: Uint8Array): ArrayBuffer {
  const copy = new Uint8Array(data.byteLength)
  copy.set(data)
  return copy.buffer
}

type ReaderItem = ChannelItem | null

/**
 * Read end of a channel: a queue of the frames the engine sends, in order.
 * The socket opens on the first `next()`. A clean close (1000, or a close
 * frame with no code) ends it; any other close, or a socket error, aborts it
 * with a {@link StreamAbortedError}, after the frames received before.
 *
 * A browser socket cannot be paused: the frames the engine sends are queued
 * here until they are read, however slow the reader (the Node SDK pauses its
 * socket above 512 KiB).
 */
export class ChannelReader {
  private ws: WebSocket | null = null
  private connected = false
  private closedLocally = false
  private ended = false
  private abortError: Error | null = null
  private readonly queue: ChannelItem[] = []
  private waiter: { resolve: (item: ReaderItem) => void; reject: (err: Error) => void } | null = null
  private readonly url: string

  constructor(engineWsBase: string, ref: StreamChannelRef) {
    this.url = buildChannelUrl(engineWsBase, ref.channel_id, ref.access_key, 'read')
  }

  /** The next frame; `null` at the end. Rejects with the abort once the frames before it were read. */
  next(): Promise<ReaderItem> {
    const item = this.queue.shift()
    if (item) return Promise.resolve(item)
    if (this.abortError) return Promise.reject(this.abortError)
    if (this.ended) return Promise.resolve(null)
    this.ensureConnected()
    return new Promise((resolve, reject) => {
      this.waiter = { resolve, reject }
    })
  }

  /**
   * Stops reading: a clean close of the socket, which tells the engine the
   * reader left (it then closes the writer). With `attach`, a socket that never
   * opened is opened for that; otherwise nothing is opened.
   */
  async cancel(attach = false): Promise<void> {
    if (this.ended || this.abortError) {
      this.ended = true
      return
    }
    this.ended = true
    this.queue.length = 0
    this.wake()
    if (!this.connected && !attach) return
    this.ensureConnected()
    this.closedLocally = true
    const ws = this.ws
    if (!ws) return
    if (ws.readyState === CONNECTING) {
      // A close during the handshake cancels it, so the engine would never see this end leave.
      await new Promise<void>((resolve) => {
        ws.addEventListener('open', () => resolve())
        ws.addEventListener('close', () => resolve())
      })
    }
    if (ws.readyState === OPEN) ws.close(1000, 'channel_close')
  }

  private ensureConnected(): void {
    if (this.connected) return
    this.connected = true
    const ws = new WebSocket(this.url)
    ws.binaryType = 'arraybuffer'
    this.ws = ws

    ws.addEventListener('message', (event: MessageEvent) => {
      if (this.ended) return
      const { data } = event
      let item: ChannelItem
      if (typeof data === 'string') item = { type: 'text', value: data }
      else if (data instanceof ArrayBuffer) item = { type: 'binary', value: new Uint8Array(data) }
      else if (ArrayBuffer.isView(data))
        item = { type: 'binary', value: new Uint8Array(data.buffer, data.byteOffset, data.byteLength) }
      else return
      if (this.waiter) {
        const { resolve } = this.waiter
        this.waiter = null
        resolve(item)
        return
      }
      this.queue.push(item)
    })

    ws.addEventListener('close', (event: CloseEvent) => {
      this.ws = null
      if (this.ended || this.abortError) return
      const { code, reason } = event
      // 1005 is a close frame with no code, which is how the engine ends a clean stream.
      if (code === 1000 || code === 1005 || this.closedLocally) {
        this.ended = true
      } else {
        // The engine's reason names the writer's close code (`channel writer aborted: closed with code 1011`).
        this.abortError = new StreamAbortedError(reason ? reason : `closed with code ${code}`)
      }
      this.wake()
    })

    ws.addEventListener('error', (event) => {
      // The reader's own close is not an error (a browser reports one for a close during the handshake).
      if (this.closedLocally || this.ended || this.abortError) return
      this.abortError = new StreamAbortedError(socketErrorText(event))
      this.wake()
    })
  }

  /** Settles a pending `next()` once the queue is empty and the stream ended or aborted. */
  private wake(): void {
    if (!this.waiter || this.queue.length > 0) return
    const { resolve, reject } = this.waiter
    if (this.abortError) {
      this.waiter = null
      reject(this.abortError)
    } else if (this.ended) {
      this.waiter = null
      resolve(null)
    }
  }
}
