import { Readable, Writable } from 'node:stream'
import { WebSocket } from 'ws'
import type { StreamChannelRef } from './iii-types'

/**
 * Write end of a streaming channel. Provides both a Node.js `Writable` stream
 * and a `sendMessage` method for sending structured text messages.
 *
 * @example
 * ```typescript
 * const channel = await iii.createChannel()
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
  private ws: WebSocket | null = null
  private wsReady = false
  private readonly pendingMessages: {
    data: Buffer | string
    callback: (err?: Error | null) => void
  }[] = []
  /** Node.js Writable stream for binary data. */
  public readonly stream: Writable
  private readonly url: string

  constructor(engineWsBase: string, ref: StreamChannelRef) {
    this.url = buildChannelUrl(engineWsBase, ref.channel_id, ref.access_key, 'write')

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
        if (this.ws) this.ws.terminate()
        callback(err)
      },
    })
  }

  private ensureConnected(): void {
    if (this.ws) return
    this.ws = new WebSocket(this.url)

    this.ws.on('open', () => {
      this.wsReady = true
      for (const { data, callback } of this.pendingMessages) {
        this.ws?.send(data, callback)
      }
      this.pendingMessages.length = 0
    })

    this.ws.on('error', (err) => {
      this.stream.destroy(err)
    })

    this.ws.on('close', () => {
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
    if (this.wsReady && this.ws) {
      this.ws.send(data, (err) => callback(err ?? null))
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
 * const channel = await iii.createChannel()
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
  private readonly messageCallbacks: Array<(msg: string) => void> = []
  /** Node.js Readable stream for binary data. */
  public readonly stream: Readable
  private readonly url: string

  constructor(engineWsBase: string, ref: StreamChannelRef) {
    this.url = buildChannelUrl(engineWsBase, ref.channel_id, ref.access_key, 'read')

    const self = this
    this.stream = new Readable({
      read() {
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
    this.ws = new WebSocket(this.url)

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

    this.ws.on('close', () => {
      this.ws = null
      if (!this.stream.destroyed) this.stream.push(null)
    })

    this.ws.on('error', (err) => {
      this.stream.destroy(err)
    })
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
  const base = engineWsBase.replace(/\/$/, '')
  return `${base}/ws/channels/${channelId}?key=${encodeURIComponent(accessKey)}&dir=${direction}`
}
