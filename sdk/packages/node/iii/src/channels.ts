import { Readable, Writable } from 'node:stream'
import { type ClientOptions, WebSocket } from 'ws'
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
 * Discriminated runtime tag for an item observed on a streaming channel.
 * Mirrors the Rust SDK's `ChannelItem` enum (`Text` / `Binary`). Carrier for
 * factory + type-guard helpers so callers can construct and discriminate
 * channel items without depending on Rust-specific shape.
 */
export type ChannelItem =
  | { type: 'text'; value: string }
  | { type: 'binary'; value: Uint8Array }

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

/**
 * Write end of a streaming channel. Provides both a Node.js `Writable` stream
 * and a `sendMessage` method for sending structured text messages.
 *
 * @example
 * ```typescript
 * import { createChannel } from 'iii-sdk/helpers'
 * const channel = await createChannel(iii)
 *
 * // Stream binary data
 * channel.writer.stream.write(Buffer.from('hello'))
 * channel.writer.stream.end()
 *
 * // Or send text messages
 * channel.writer.sendMessage(JSON.stringify({ type: 'event', data: 'test' }))
 * channel.writer.close()
 * ```
 */
export class ChannelWriter {
  private static readonly FRAME_SIZE = 64 * 1024
  // A stalled reader stops the engine reading this socket; ws's 30 s default would then drop the queued clean
  // close, which the engine reads as an abort. Same bound as the engine's channel TTL.
  private static readonly CLOSE_TIMEOUT_MS = 5 * 60 * 1000
  private ws: WebSocket | null = null
  private wsReady = false
  private readonly pendingMessages: {
    data: Buffer | string
    callback: (err?: Error | null) => void
  }[] = []
  /** Node.js Writable stream for binary data. */
  public readonly stream: Writable
  private readonly url: string
  private readonly headers?: Record<string, string>

  constructor(engineWsBase: string, ref: StreamChannelRef, headers?: Record<string, string>) {
    this.url = buildChannelUrl(engineWsBase, ref.channel_id, ref.access_key, 'write')
    this.headers = resolveHeaders(headers)

    this.stream = new Writable({
      write: (chunk: Buffer, _encoding, callback) => {
        const buf = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)
        this.sendChunked(buf, callback)
      },
      final: (callback) => {
        if (!this.ws) {
          callback()
          return
        }
        // Delay the close frame slightly to allow the TCP stack to flush
        // all buffered send() data. Without this, the close frame can arrive
        // at the engine before all data frames, causing data truncation.
        const doClose = () => {
          if (this.ws) {
            this.ws.close(1000, 'stream_complete')
          }
          callback()
        }
        if (this.wsReady) {
          setTimeout(doClose, 10)
        } else {
          this.ws.on('open', () => setTimeout(doClose, 10))
        }
      },
      destroy: (err, callback) => {
        // autoDestroy runs right after end()'s close(1000): terminating then would drop a queued close frame.
        if (this.ws && (err || this.ws.readyState !== WebSocket.CLOSING)) this.ws.terminate()
        callback(err)
      },
    })
  }

  private ensureConnected(): void {
    // A non-null socket is only reusable while it is still connecting or open.
    // A CLOSING/CLOSED socket is dead and must be replaced — guarding solely on
    // `this.ws` being non-null would pin a terminated socket forever, so every
    // subsequent send would target a connection that can never deliver it.
    if (
      this.ws &&
      this.ws.readyState !== WebSocket.CLOSING &&
      this.ws.readyState !== WebSocket.CLOSED
    ) {
      return
    }
    this.wsReady = false
    // @types/ws 8.18 does not declare ws 8.21's `closeTimeout` client option yet.
    const options: ClientOptions & { closeTimeout: number } = {
      handshakeTimeout: WS_HANDSHAKE_TIMEOUT_MS,
      closeTimeout: ChannelWriter.CLOSE_TIMEOUT_MS,
      headers: this.headers,
    }
    this.ws = new WebSocket(this.url, options)

    this.ws.on('open', () => {
      this.wsReady = true
      for (const { data, callback } of this.pendingMessages) {
        this.ws?.send(data, callback)
      }
      this.pendingMessages.length = 0
    })

    this.ws.on('error', (err) => {
      this.wsReady = false
      this.stream.destroy(err)
    })

    this.ws.on('close', () => {
      this.wsReady = false
      if (!this.stream.destroyed) {
        this.stream.destroy()
      }
    })
  }

  /** Send a text message through the channel. */
  sendMessage(msg: string): void {
    this.ensureConnected()
    this.sendRaw(msg, (err) => {
      if (err) this.stream.destroy(err)
    })
  }

  /** Close the channel writer. */
  close(): void {
    if (!this.ws) return
    const doClose = () => {
      if (this.ws) {
        this.ws.close(1000, 'channel_close')
      }
    }
    if (this.wsReady) {
      doClose()
    } else {
      this.ws.on('open', () => doClose())
    }
  }

  private sendChunked(data: Buffer, callback: (err?: Error | null) => void): void {
    let offset = 0
    const sendNext = (err?: Error | null): void => {
      if (err) {
        callback(err)
        return
      }

      if (offset >= data.length) {
        callback(null)
        return
      }

      const end = Math.min(offset + ChannelWriter.FRAME_SIZE, data.length)
      const part = data.subarray(offset, end)
      offset = end
      this.sendRaw(part, sendNext)
    }
    sendNext(null)
  }

  private sendRaw(data: Buffer | string, callback: (err?: Error | null) => void): void {
    this.ensureConnected()
    // Trust the socket's live readyState over the tracked `wsReady` flag, which
    // can stay true after a drop. Queue anything we cannot send right now — and
    // requeue a send() that throws because the socket died underneath us — so it
    // flushes on the next open instead of being lost or throwing to the caller.
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      try {
        this.ws.send(data, (err) => callback(err ?? null))
      } catch {
        this.pendingMessages.push({ data, callback })
      }
    } else {
      this.pendingMessages.push({ data, callback })
    }
  }
}

/**
 * Read end of a streaming channel. Provides both a Node.js `Readable` stream
 * for binary data and an `onMessage` callback for structured text messages.
 *
 * @example
 * ```typescript
 * import { createChannel } from 'iii-sdk/helpers'
 * const channel = await createChannel(iii)
 *
 * // Stream binary data
 * channel.reader.stream.on('data', (chunk) => console.log(chunk))
 *
 * // Or receive text messages
 * channel.reader.onMessage((msg) => console.log('Got:', msg))
 * ```
 */
export class ChannelReader {
  private ws: WebSocket | null = null
  private connected = false
  private closedLocally = false
  private abortError: Error | null = null
  private readonly messageCallbacks: Array<(msg: string) => void> = []
  /** Node.js Readable stream for binary data. */
  public readonly stream: Readable
  private readonly url: string
  private readonly headers?: Record<string, string>

  constructor(engineWsBase: string, ref: StreamChannelRef, headers?: Record<string, string>) {
    this.url = buildChannelUrl(engineWsBase, ref.channel_id, ref.access_key, 'read')
    this.headers = resolveHeaders(headers)

    const self = this
    this.stream = new Readable({
      read() {
        if (self.abortError) {
          self.abort(self.abortError)
          return
        }
        self.ensureConnected()
        if (self.ws) self.ws.resume()
      },
      destroy(err, callback) {
        if (self.ws && self.ws.readyState !== WebSocket.CLOSED) {
          self.ws.terminate()
        }
        self.ws = null
        callback(err)
      },
    })
  }

  private ensureConnected(): void {
    if (this.connected) return
    this.connected = true
    this.ws = new WebSocket(this.url, { handshakeTimeout: WS_HANDSHAKE_TIMEOUT_MS, headers: this.headers })

    this.ws.on('open', () => {
      ;(this.ws as unknown as { binaryType: string }).binaryType = 'nodebuffer'
    })

    this.ws.on('message', (data: Buffer, isBinary: boolean) => {
      if (isBinary) {
        if (!this.stream.push(data)) {
          this.ws?.pause()
        }
      } else {
        const msg = data.toString('utf-8')
        for (const cb of this.messageCallbacks) {
          cb(msg)
        }
      }
    })

    this.ws.on('close', (code: number, reason: Buffer) => {
      this.ws = null
      if (this.stream.destroyed || this.abortError) return
      // 1005 is a close frame with no code, which is how the engine ends a clean stream.
      if (code === 1000 || code === 1005 || this.closedLocally) {
        this.stream.push(null)
      } else {
        this.abort(new Error(`channel aborted (close ${code}${reason.length ? `: ${reason}` : ''})`))
      }
    })

    this.ws.on('error', (err) => {
      // ws emits this when close() cuts a handshake short; the reader's own close is not an error.
      if (!this.closedLocally) this.abort(err)
    })
  }

  // Data the engine queued before the abort is delivered first: the error waits until the buffer drains.
  // shortcut: a read(n) consumer whose last block is shorter than n never sees the abort; upgrade if one appears.
  private abort(err: Error): void {
    this.abortError ??= err
    if (this.stream.readableLength === 0) this.stream.destroy(this.abortError)
    // An empty push ends the pending read, so the stream calls read() again once the consumer drains it.
    else this.stream.push(Buffer.alloc(0))
  }

  /** Register a callback to receive text messages from the channel. */
  onMessage(callback: (msg: string) => void): void {
    this.messageCallbacks.push(callback)
  }

  async readAll(): Promise<Buffer> {
    this.ensureConnected()
    const chunks: Buffer[] = []

    for await (const chunk of this.stream) {
      chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
    }

    return Buffer.concat(chunks)
  }

  close(): void {
    if (this.ws && this.ws.readyState !== WebSocket.CLOSED) {
      this.closedLocally = true
      this.ws.close(1000, 'channel_close')
    }
  }
}

function buildChannelUrl(
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
