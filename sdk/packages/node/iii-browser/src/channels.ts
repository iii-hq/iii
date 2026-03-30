import type { StreamChannelRef } from './iii-types'

/**
 * Write end of a streaming channel. Uses native browser WebSocket.
 *
 * @example
 * ```typescript
 * const channel = await iii.createChannel()
 *
 * channel.writer.sendMessage(JSON.stringify({ type: 'event', data: 'test' }))
 * channel.writer.sendBinary(new Uint8Array([1, 2, 3]))
 * channel.writer.close()
 * ```
 */
export class ChannelWriter {
  private static readonly FRAME_SIZE = 64 * 1024
  private ws: WebSocket | null = null
  private wsReady = false
  private readonly pendingMessages: {
    data: ArrayBuffer | string
    resolve: () => void
    reject: (err: Error) => void
  }[] = []
  private readonly url: string

  constructor(engineWsBase: string, ref: StreamChannelRef) {
    this.url = buildChannelUrl(engineWsBase, ref.channel_id, ref.access_key, 'write')
  }

  private ensureConnected(): void {
    if (this.ws) return

    this.ws = new WebSocket(this.url)
    this.ws.binaryType = 'arraybuffer'

    this.ws.addEventListener('open', () => {
      this.wsReady = true
      for (const { data, resolve, reject } of this.pendingMessages) {
        try {
          this.ws?.send(data)
          resolve()
        } catch (err) {
          reject(err instanceof Error ? err : new Error(String(err)))
        }
      }
      this.pendingMessages.length = 0
    })

    this.ws.addEventListener('error', () => {
      for (const { reject } of this.pendingMessages) {
        reject(new Error('WebSocket error'))
      }
      this.pendingMessages.length = 0
    })
  }

  /** Send a text message through the channel. */
  sendMessage(msg: string): void {
    this.ensureConnected()
    this.sendRaw(msg)
  }

  /** Send binary data through the channel. */
  sendBinary(data: Uint8Array): void {
    this.ensureConnected()

    let offset = 0
    while (offset < data.length) {
      const end = Math.min(offset + ChannelWriter.FRAME_SIZE, data.length)
      const chunk = data.subarray(offset, end)
      const buffer = chunk.buffer instanceof ArrayBuffer
        ? chunk.buffer
        : new ArrayBuffer(chunk.byteLength)
      if (!(chunk.buffer instanceof ArrayBuffer)) {
        new Uint8Array(buffer).set(chunk)
      }
      this.sendRaw(buffer.slice(chunk.byteOffset, chunk.byteOffset + chunk.byteLength))
      offset = end
    }
  }

  /** Close the channel writer. */
  close(): void {
    if (!this.ws) return

    const doClose = () => {
      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        this.ws.close(1000, 'channel_close')
      }
    }

    if (this.wsReady) {
      doClose()
    } else {
      this.ws.addEventListener('open', () => doClose())
    }
  }

  private sendRaw(data: ArrayBuffer | string): void {
    if (this.wsReady && this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(data)
    } else {
      this.ensureConnected()
      this.pendingMessages.push({
        data,
        resolve: () => {},
        reject: () => {},
      })
    }
  }
}

/**
 * Read end of a streaming channel. Uses native browser WebSocket.
 *
 * @example
 * ```typescript
 * const channel = await iii.createChannel()
 *
 * channel.reader.onMessage((msg) => console.log('Got:', msg))
 * channel.reader.onBinary((data) => console.log('Binary:', data.byteLength))
 * ```
 */
export class ChannelReader {
  private ws: WebSocket | null = null
  private connected = false
  private readonly messageCallbacks: Array<(msg: string) => void> = []
  private readonly binaryCallbacks: Array<(data: Uint8Array) => void> = []
  private readonly url: string

  constructor(engineWsBase: string, ref: StreamChannelRef) {
    this.url = buildChannelUrl(engineWsBase, ref.channel_id, ref.access_key, 'read')
  }

  private ensureConnected(): void {
    if (this.connected) return
    this.connected = true

    this.ws = new WebSocket(this.url)
    this.ws.binaryType = 'arraybuffer'

    this.ws.addEventListener('message', (event: MessageEvent) => {
      if (event.data instanceof ArrayBuffer) {
        const data = new Uint8Array(event.data)
        for (const cb of this.binaryCallbacks) {
          cb(data)
        }
      } else if (typeof event.data === 'string') {
        for (const cb of this.messageCallbacks) {
          cb(event.data)
        }
      }
    })

    this.ws.addEventListener('close', () => {
      this.ws = null
    })

    this.ws.addEventListener('error', () => {
      this.ws = null
    })
  }

  /** Register a callback to receive text messages from the channel. */
  onMessage(callback: (msg: string) => void): void {
    this.messageCallbacks.push(callback)
    this.ensureConnected()
  }

  /** Register a callback to receive binary data from the channel. */
  onBinary(callback: (data: Uint8Array) => void): void {
    this.binaryCallbacks.push(callback)
    this.ensureConnected()
  }

  /** Read all binary data from the channel until it closes. */
  async readAll(): Promise<Uint8Array> {
    this.ensureConnected()
    const chunks: Uint8Array[] = []

    return new Promise<Uint8Array>((resolve) => {
      const onData = (data: Uint8Array) => {
        chunks.push(data)
      }
      this.binaryCallbacks.push(onData)

      const originalWs = this.ws
      if (originalWs) {
        originalWs.addEventListener('close', () => {
          const totalLength = chunks.reduce((sum, c) => sum + c.length, 0)
          const result = new Uint8Array(totalLength)
          let offset = 0
          for (const chunk of chunks) {
            result.set(chunk, offset)
            offset += chunk.length
          }
          resolve(result)
        })
      }
    })
  }

  /** Close the channel reader. */
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
