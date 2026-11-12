/**
 * The two WebSocket ends of an engine channel (`/ws/channels/<id>`), the
 * transport under {@link InputStream} and {@link OutputStream}. Internal: the
 * public API is the stream types; the low-level wire names are in
 * `iii-sdk/protocol`.
 */
import { type ClientOptions, WebSocket } from 'ws'
import { InvocationError, StreamAbortedError } from './errors'
import { WS_HANDSHAKE_TIMEOUT_MS } from './iii-constants'
import type { StreamChannelRef } from './iii-types'
import { resolveHeaders } from './utils'

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
 * A stalled reader stops the engine reading the writer's socket; ws's 30 s
 * default would then drop the queued clean close, which the engine reads as an
 * abort. Same bound as the engine's channel TTL.
 */
const CLOSE_TIMEOUT_MS = 5 * 60 * 1000
/**
 * Delay between the last data frame and the close frame, so the TCP stack
 * flushes the data first: without it the close frame can reach the engine
 * before the data, which truncates the stream.
 */
const CLOSE_DELAY_MS = 10
/** The close code an abort sends: the engine relays any code but 1000 to the reader as an abort. */
export const ABORT_CLOSE_CODE = 1011
/** A WebSocket close reason holds at most 123 bytes. */
const MAX_CLOSE_REASON_BYTES = 123
/** The reader pauses its socket above this many queued bytes, and resumes below half of it. */
const READ_HIGH_WATER_BYTES = 512 * 1024

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
  const bytes = Buffer.from(reason, 'utf8')
  if (bytes.length <= MAX_CLOSE_REASON_BYTES) return reason
  let cut = MAX_CLOSE_REASON_BYTES
  while (cut > 0 && (bytes[cut] & 0xc0) === 0x80) cut--
  return bytes.subarray(0, cut).toString('utf8')
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

/** A writer's socket failed (it could not connect, or the engine reset it): no more data goes through. */
function socketFailed(err: Error): InvocationError {
  return streamClosedError(`the stream's connection failed: ${err.message}`, err)
}

type Pending = { data: Buffer | string; resolve: () => void; reject: (err: Error) => void }

/**
 * Write end of a channel. The socket opens on the first operation; what is
 * sent before it opens is queued. Binary data goes out in 64 KiB frames, text
 * as one text frame. `close()` ends the stream cleanly (code 1000, after a
 * short flush delay); `abort()` closes with {@link ABORT_CLOSE_CODE}, which
 * the reader gets as an abort. Both open the socket first when it never
 * opened, so the reader always learns how the stream ended. Once the socket
 * is gone (the engine closes it when the reader leaves), every operation fails.
 */
export class ChannelWriter {
  private ws: WebSocket | null = null
  private wsReady = false
  /** Why the writer can take no more data: set once, when it fails or ends. */
  private failure: Error | null = null
  /** Set once close() or abort() put (or will put) a close frame on the wire. */
  private ending: Promise<void> | null = null
  private readonly pendingMessages: Pending[] = []
  private readonly url: string
  private readonly headers?: Record<string, string>

  constructor(engineWsBase: string, ref: StreamChannelRef, headers?: Record<string, string>) {
    this.url = buildChannelUrl(engineWsBase, ref.channel_id, ref.access_key, 'write')
    this.headers = resolveHeaders(headers)
  }

  /** Sends `data` in binary frames; resolves once the socket took the last one. */
  writeBinary(data: Uint8Array): Promise<void> {
    const buf = Buffer.isBuffer(data) ? data : Buffer.from(data.buffer, data.byteOffset, data.byteLength)
    if (buf.length <= FRAME_SIZE) return this.send(buf)
    const parts: Promise<void>[] = []
    for (let offset = 0; offset < buf.length; offset += FRAME_SIZE) {
      parts.push(this.send(buf.subarray(offset, Math.min(offset + FRAME_SIZE, buf.length))))
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
    this.ending = this.finish(1000, 'stream_complete', CLOSE_DELAY_MS)
    return this.ending
  }

  /** Aborts the stream: the reader gets an error instead of an end. Resolves once the socket closed. */
  abort(reason: string): Promise<void> {
    if (this.ending) return this.ending
    this.ending = this.finish(ABORT_CLOSE_CODE, closeReason(reason || 'aborted'), 0)
    return this.ending
  }

  private send(data: Buffer | string): Promise<void> {
    if (this.failure) return Promise.reject(this.failure)
    if (this.ending) return Promise.reject(streamClosedError('the stream was already closed'))
    return new Promise<void>((resolve, reject) => this.sendRaw({ data, resolve, reject }))
  }

  private async finish(code: number, reason: string, delayMs: number): Promise<void> {
    if (this.failure) return
    this.ensureConnected()
    const ws = this.ws as WebSocket
    // A close sent while the handshake runs cancels it: the engine never sees this end attach.
    if (!(await this.opened(ws))) return
    if (delayMs > 0) await new Promise((resolve) => setTimeout(resolve, delayMs))
    if (ws.readyState === WebSocket.CLOSED) return
    await new Promise<void>((resolve) => {
      ws.once('close', () => resolve())
      ws.close(code, reason)
    })
  }

  /** Resolves `true` once `ws` is open, `false` when it closed or failed first. */
  private opened(ws: WebSocket): Promise<boolean> {
    if (ws.readyState === WebSocket.OPEN) return Promise.resolve(true)
    if (ws.readyState !== WebSocket.CONNECTING) return Promise.resolve(false)
    return new Promise((resolve) => {
      const done = (ok: boolean) => {
        ws.off('open', onOpen)
        ws.off('close', onClose)
        resolve(ok)
      }
      const onOpen = () => done(true)
      const onClose = () => done(false)
      ws.on('open', onOpen)
      ws.on('close', onClose)
    })
  }

  private ensureConnected(): void {
    if (this.ws) return
    this.wsReady = false
    // @types/ws 8.18 does not declare ws 8.21's `closeTimeout` client option yet.
    const options: ClientOptions & { closeTimeout: number } = {
      handshakeTimeout: WS_HANDSHAKE_TIMEOUT_MS,
      closeTimeout: CLOSE_TIMEOUT_MS,
      headers: this.headers,
    }
    const ws = new WebSocket(this.url, options)
    this.ws = ws

    ws.on('open', () => {
      this.wsReady = true
      const queued = this.pendingMessages.splice(0)
      for (const item of queued) this.sendRaw(item)
    })

    ws.on('error', (err) => {
      this.wsReady = false
      this.fail(socketFailed(err))
    })

    ws.on('close', () => {
      this.wsReady = false
      this.fail(this.closedError())
    })
  }

  /** Our own close is the end we asked for; any other close means nobody reads what is sent now. */
  private closedError(): Error {
    return streamClosedError(this.ending ? 'the stream was already closed' : 'the reader closed the stream')
  }

  private fail(err: Error): void {
    this.failure ??= err
    const queued = this.pendingMessages.splice(0)
    for (const item of queued) item.reject(this.failure)
  }

  private sendRaw(item: Pending): void {
    if (this.failure) {
      item.reject(this.failure)
      return
    }
    this.ensureConnected()
    // Trust the socket's live readyState over the tracked `wsReady` flag, which can stay true after a drop.
    // Queue what cannot go out yet, and a send() that throws because the socket died underneath us: the
    // socket's close or error then fails it instead of the caller getting a throw.
    const ws = this.ws
    if (ws && this.wsReady && ws.readyState === WebSocket.OPEN) {
      try {
        ws.send(item.data, (err) => {
          if (!err) item.resolve()
          // A socket the engine closed (the reader left) fails its last writes with a socket error.
          else item.reject(this.failure ?? (ws.readyState === WebSocket.OPEN ? socketFailed(err) : this.closedError()))
        })
        return
      } catch {
        // falls through to the queue
      }
    }
    this.pendingMessages.push(item)
  }
}

type ReaderItem = ChannelItem | null

/**
 * Read end of a channel: a queue of the frames the engine sends, in order.
 * The socket opens on the first `next()`. A clean close (1000, or a close
 * frame with no code) ends it; any other close, or a socket error, aborts it
 * with a {@link StreamAbortedError}, after the frames received before. The
 * socket pauses while the queue holds more than {@link READ_HIGH_WATER_BYTES}.
 */
export class ChannelReader {
  private ws: WebSocket | null = null
  private connected = false
  private closedLocally = false
  private ended = false
  private abortError: Error | null = null
  private readonly queue: ChannelItem[] = []
  private queuedBytes = 0
  private paused = false
  private waiter: { resolve: (item: ReaderItem) => void; reject: (err: Error) => void } | null = null
  private readonly url: string
  private readonly headers?: Record<string, string>

  constructor(engineWsBase: string, ref: StreamChannelRef, headers?: Record<string, string>) {
    this.url = buildChannelUrl(engineWsBase, ref.channel_id, ref.access_key, 'read')
    this.headers = resolveHeaders(headers)
  }

  /** The next frame; `null` at the end. Rejects with the abort once the frames before it were read. */
  next(): Promise<ReaderItem> {
    const item = this.queue.shift()
    if (item) {
      this.queuedBytes -= frameBytes(item)
      if (this.paused && this.queuedBytes < READ_HIGH_WATER_BYTES / 2) {
        this.paused = false
        this.ws?.resume()
      }
      return Promise.resolve(item)
    }
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
    this.queuedBytes = 0
    this.wake()
    if (!this.connected && !attach) return
    this.ensureConnected()
    this.closedLocally = true
    const ws = this.ws
    if (!ws) return
    if (ws.readyState === WebSocket.CONNECTING) {
      // A close during the handshake cancels it, so the engine would never see this end leave.
      await new Promise<void>((resolve) => {
        ws.once('open', () => resolve())
        ws.once('close', () => resolve())
      })
    }
    if (ws.readyState === WebSocket.OPEN) ws.close(1000, 'channel_close')
  }

  private ensureConnected(): void {
    if (this.connected) return
    this.connected = true
    const ws = new WebSocket(this.url, { handshakeTimeout: WS_HANDSHAKE_TIMEOUT_MS, headers: this.headers })
    this.ws = ws

    ws.on('open', () => {
      ;(ws as unknown as { binaryType: string }).binaryType = 'nodebuffer'
    })

    ws.on('message', (data: Buffer, isBinary: boolean) => {
      if (this.ended) return
      const item: ChannelItem = isBinary
        ? { type: 'binary', value: data }
        : { type: 'text', value: data.toString('utf-8') }
      if (this.waiter) {
        const { resolve } = this.waiter
        this.waiter = null
        resolve(item)
        return
      }
      this.queue.push(item)
      this.queuedBytes += frameBytes(item)
      if (!this.paused && this.queuedBytes >= READ_HIGH_WATER_BYTES) {
        this.paused = true
        ws.pause()
      }
    })

    ws.on('close', (code: number, reason: Buffer) => {
      this.ws = null
      if (this.ended || this.abortError) return
      // 1005 is a close frame with no code, which is how the engine ends a clean stream.
      if (code === 1000 || code === 1005 || this.closedLocally) {
        this.ended = true
      } else {
        // The engine's reason names the writer's close code (`channel writer aborted: closed with code 1011`).
        this.abortError = new StreamAbortedError(reason.length ? reason.toString('utf-8') : `closed with code ${code}`)
      }
      this.wake()
    })

    ws.on('error', (err) => {
      // ws emits this when close() cuts a handshake short; the reader's own close is not an error.
      if (this.closedLocally || this.ended || this.abortError) return
      this.abortError = new StreamAbortedError(err.message, { cause: err })
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

function frameBytes(item: ChannelItem): number {
  return item.type === 'binary' ? item.value.byteLength : item.value.length
}
