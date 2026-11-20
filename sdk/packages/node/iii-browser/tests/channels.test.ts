import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { inputFromRef, outputFromRef } from '../src/call-streams'
import { ABORT_CLOSE_CODE, buildChannelUrl, closeReason } from '../src/channels'
import { StreamAbortedError } from '../src/errors'
import type { StreamChannelRef } from '../src/iii-types'
import { MockEngine } from './mock-websocket'

// The stream ends on the browser's own WebSocket, played by a mock that keeps a
// browser's rules (close codes 1000 and 3000-4999, reasons up to 123 bytes).

function makeRef(direction: 'read' | 'write'): StreamChannelRef {
  return {
    channel_id: 'ch-test-123',
    access_key: 'key-abc',
    direction,
  }
}

const binding = { wsBase: 'ws://engine:49135' }
const tick = () => new Promise((resolve) => setTimeout(resolve, 0))
const until = async (check: () => boolean) => {
  for (let i = 0; i < 200 && !check(); i++) await tick()
}

describe('Stream ends on a browser WebSocket', () => {
  let engine: MockEngine

  beforeEach(() => {
    engine = new MockEngine({ autoOpen: false })
    engine.install()
  })

  afterEach(() => {
    engine.uninstall()
  })

  describe('OutputStream on a write ref', () => {
    it('connects on its first operation and sends a message as a text frame', async () => {
      const out = outputFromRef(makeRef('write'), binding)
      const sent = out.send('hello world')
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      expect(socket).toBeDefined()
      expect(socket.url).toContain('/ws/channels/ch-test-123')
      expect(socket.url).toContain('key=key-abc')
      expect(socket.url).toContain('dir=write')
      expect(socket.binaryType).toBe('arraybuffer')

      socket.simulateOpen()
      await sent

      expect(socket.sentRaw).toContain('"hello world"')
    })

    it('sends binary data in frames of at most 64 KiB, each in an ArrayBuffer of its own', async () => {
      const out = outputFromRef(makeRef('write'), binding)

      const data = new Uint8Array(64 * 1024 + 100)
      data.fill(42)

      const written = out.write(data)
      await until(() => engine.sockets.length > 0)
      const socket = engine.sockets[0]
      socket.simulateOpen()
      await written

      const binarySent = socket.sentRaw.filter((d) => d instanceof ArrayBuffer)
      expect(binarySent.length).toBe(2)
      expect((binarySent[0] as ArrayBuffer).byteLength).toBe(64 * 1024)
      expect((binarySent[1] as ArrayBuffer).byteLength).toBe(100)
      expect(new Uint8Array(binarySent[1] as ArrayBuffer).every((b) => b === 42)).toBe(true)
    })

    it('holds what is written before the socket opens, and sends it in order once it does', async () => {
      const out = outputFromRef(makeRef('write'), binding)
      const first = out.send('msg-1')
      const second = out.write('msg-2')
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      await tick()
      expect(socket.sentRaw).toHaveLength(0)

      socket.simulateOpen()
      await Promise.all([first, second])

      expect(socket.sentRaw[0]).toBe('"msg-1"')
      expect(new TextDecoder().decode(socket.sentRaw[1] as ArrayBuffer)).toBe('msg-2')
    })

    it('closes with code 1000 and stream_complete after the data, and resolves on the close event', async () => {
      const out = outputFromRef(makeRef('write'), binding)
      const sent = out.send('init')
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      socket.simulateOpen()
      await sent

      let closed = false
      const closing = out.close().then(() => {
        closed = true
      })
      await until(() => socket.closeCode !== undefined)

      expect(socket.closeCode).toBe(1000)
      expect(socket.closeReason).toBe('stream_complete')
      expect(closed).toBe(false)
      socket.simulateClose(1000)
      await closing
    })

    it('aborts with a close code a browser accepts (4011), and a reason cut to 123 bytes', async () => {
      const out = outputFromRef(makeRef('write'), binding)
      const long = 'é'.repeat(100)
      const aborting = out.abort(long)
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      socket.simulateOpen()
      await until(() => socket.closeCode !== undefined)

      expect(ABORT_CLOSE_CODE).toBe(4011)
      expect(socket.closeCode).toBe(4011)
      expect(socket.closeReason).toBe(closeReason(long))
      expect(new TextEncoder().encode(socket.closeReason).byteLength).toBeLessThanOrEqual(123)
      socket.simulateClose(4011)
      await aborting
    })

    it('fails a write with STREAM_CLOSED once the socket closed under it (the reader left)', async () => {
      const out = outputFromRef(makeRef('write'), binding)
      const first = out.write('first')
      await until(() => engine.sockets.length > 0)
      const socket = engine.sockets[0]
      socket.simulateOpen()
      await first
      socket.simulateClose(1000)

      await expect(out.write('second')).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
    })

    it('a write resolves only once the socket holds at most 1 MiB unsent (the browser backpressure)', async () => {
      const out = outputFromRef(makeRef('write'), binding)
      const first = out.write('a')
      await until(() => engine.sockets.length > 0)
      const socket = engine.sockets[0]
      socket.simulateOpen()
      await first

      // The socket has not sent what it holds: the next write waits.
      socket.bufferedAmount = 2 * 1024 * 1024
      let resolved = false
      const second = out.write('b').then(() => {
        resolved = true
      })
      await new Promise((r) => setTimeout(r, 30))
      expect(socket.sentRaw).toHaveLength(2)
      expect(resolved).toBe(false)

      socket.bufferedAmount = 512 * 1024
      await second
      expect(resolved).toBe(true)
    })

    it('close() waits for the socket to drain, then sends its close frame', async () => {
      const out = outputFromRef(makeRef('write'), binding)
      const first = out.write('a')
      await until(() => engine.sockets.length > 0)
      const socket = engine.sockets[0]
      socket.simulateOpen()
      await first

      socket.bufferedAmount = 100
      const closing = out.close()
      await new Promise((r) => setTimeout(r, 40))
      expect(socket.closeCode).toBeUndefined()

      socket.bufferedAmount = 0
      await until(() => socket.closeCode !== undefined)
      expect(socket.closeCode).toBe(1000)
      socket.simulateClose(1000)
      await closing
    })

    it('a write whose frame the socket still held when it closed fails with STREAM_CLOSED', async () => {
      const out = outputFromRef(makeRef('write'), binding)
      const first = out.write('a')
      await until(() => engine.sockets.length > 0)
      const socket = engine.sockets[0]
      socket.simulateOpen()
      await first

      socket.bufferedAmount = 2 * 1024 * 1024
      const stuck = out.write('b')
      await new Promise((r) => setTimeout(r, 20))
      socket.simulateClose(1000)
      await expect(stuck).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
    })

    it('abort() returns at once while a write waits on backpressure, and that write fails with STREAM_CLOSED', async () => {
      const out = outputFromRef(makeRef('write'), binding)
      const first = out.write('a')
      await until(() => engine.sockets.length > 0)
      const socket = engine.sockets[0]
      socket.simulateOpen()
      await first

      // The socket holds more than 1 MiB unsent and never drains: the next write waits.
      socket.bufferedAmount = 2 * 1024 * 1024
      const held = out.write('b')
      held.catch(() => undefined)
      await new Promise((r) => setTimeout(r, 20))
      const started = performance.now()
      await out.abort('stop')
      expect(performance.now() - started).toBeLessThan(100)
      await expect(held).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
      await until(() => socket.closeCode !== undefined)
      expect(socket.closeCode).toBe(4011)
    })

    it('fails its writes with STREAM_CLOSED when the socket never opens', async () => {
      const out = outputFromRef(makeRef('write'), binding)
      const write = out.write('lost')
      await until(() => engine.sockets.length > 0)
      const socket = engine.sockets[0]
      socket.simulateError()
      socket.simulateClose(1006)

      await expect(write).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
    })
  })

  describe('InputStream on a read ref', () => {
    it('reads text messages with messages()', async () => {
      const input = inputFromRef(makeRef('read'), binding)
      const received = (async () => {
        const out: unknown[] = []
        for await (const msg of input.messages()) out.push(msg)
        return out
      })()
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      expect(socket).toBeDefined()
      expect(socket.url).toContain('dir=read')

      socket.simulateOpen()
      socket.simulateMessage('text-message-1')
      socket.simulateMessage('{"n":2}')
      socket.simulateClose(1000)

      expect(await received).toEqual(['text-message-1', { n: 2 }])
    })

    it('reads binary data chunk by chunk, as Uint8Arrays', async () => {
      const input = inputFromRef(makeRef('read'), binding)
      const it = input[Symbol.asyncIterator]()
      const first = it.next()
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      socket.simulateOpen()

      const buffer = new ArrayBuffer(4)
      new Uint8Array(buffer).set([1, 2, 3, 4])
      socket.simulateMessage(buffer)

      const { value } = await first
      expect(value).toBeInstanceOf(Uint8Array)
      expect(value).toEqual(new Uint8Array([1, 2, 3, 4]))
      await it.return?.()
    })

    it('collects all binary data with bytes() until the close', async () => {
      const input = inputFromRef(makeRef('read'), binding)
      const readPromise = input.bytes()
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      socket.simulateOpen()

      const buf1 = new ArrayBuffer(3)
      new Uint8Array(buf1).set([10, 20, 30])
      socket.simulateMessage(buf1)

      const buf2 = new ArrayBuffer(2)
      new Uint8Array(buf2).set([40, 50])
      socket.simulateMessage(buf2)

      socket.simulateClose()

      const result = await readPromise
      expect(result).toEqual(new Uint8Array([10, 20, 30, 40, 50]))
    })

    it('ends cleanly on a close with no status code (1005)', async () => {
      const input = inputFromRef(makeRef('read'), binding)
      const readPromise = input.bytes()
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      socket.simulateOpen()
      socket.simulateMessage(new Uint8Array([1, 2]).buffer)
      socket.simulateClose(1005)

      expect(await readPromise).toEqual(new Uint8Array([1, 2]))
    })

    it('rejects with STREAM_ABORTED and the engine reason when the writer aborts (1011)', async () => {
      const input = inputFromRef(makeRef('read'), binding)
      const readPromise = input.bytes()
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      socket.simulateOpen()
      socket.simulateMessage(new Uint8Array([1, 2]).buffer)
      socket.simulateClose(1011, 'channel writer aborted: connection lost before close')

      const error = await readPromise.catch((e: unknown) => e)
      expect(error).toBeInstanceOf(StreamAbortedError)
      expect(error).toMatchObject({
        code: 'STREAM_ABORTED',
        reason: 'channel writer aborted: connection lost before close',
        message: 'STREAM_ABORTED: stream aborted: channel writer aborted: connection lost before close',
      })
    })

    it('rejects with STREAM_ABORTED when the connection drops without a close frame (1006)', async () => {
      const input = inputFromRef(makeRef('read'), binding)
      const readPromise = input.bytes()
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      socket.simulateOpen()
      socket.simulateError()
      socket.simulateClose(1006)

      await expect(readPromise).rejects.toMatchObject({ code: 'STREAM_ABORTED' })
    })

    it('treats its own cancel as a cancel, not an abort, whatever close code follows', async () => {
      const input = inputFromRef(makeRef('read'), binding)
      const it = input[Symbol.asyncIterator]()
      const first = it.next()
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      socket.simulateOpen()
      socket.simulateMessage(new Uint8Array([7]).buffer)
      expect((await first).value).toEqual(new Uint8Array([7]))
      await input.cancel()
      socket.simulateClose(1006)

      await expect(it.next()).rejects.toMatchObject({
        code: 'STREAM_CLOSED',
        message: 'STREAM_CLOSED: the stream was cancelled',
      })
    })

    it('cancel() on a stream never read opens it, then closes it with 1000 (the writer learns the reader left)', async () => {
      const input = inputFromRef(makeRef('read'), binding)
      const cancelled = input.cancel()
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      expect(socket.closeCode).toBeUndefined()
      socket.simulateOpen()
      await cancelled

      expect(socket.closeCode).toBe(1000)
      expect(socket.closeReason).toBe('channel_close')
    })

    it('cancel() closes an open reader with code 1000', async () => {
      const input = inputFromRef(makeRef('read'), binding)
      const messages = input.messages()
      const next = messages.next()
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      socket.simulateOpen()
      socket.simulateMessage('one')
      await next

      await input.cancel()

      expect(socket.closeCode).toBe(1000)
      expect(socket.closeReason).toBe('channel_close')
    })
  })

  describe('URL construction', () => {
    it('builds the channel URL, with the key escaped', async () => {
      const out = outputFromRef(
        { channel_id: 'my-channel', access_key: 'secret key&value', direction: 'write' },
        binding,
      )
      void out.send('init')
      await until(() => engine.sockets.length > 0)

      const socket = engine.sockets[0]
      expect(socket.url).toBe('ws://engine:49135/ws/channels/my-channel?key=secret%20key%26value&dir=write')
    })

    it('keeps a query of the engine URL (a browser authenticates there, it cannot send headers)', () => {
      expect(buildChannelUrl('ws://engine:49135/?token=abc', 'c1', 'k', 'read')).toBe(
        'ws://engine:49135/ws/channels/c1?token=abc&key=k&dir=read',
      )
      expect(buildChannelUrl('ws://engine:49135', 'c1', 'k', 'write')).toBe(
        'ws://engine:49135/ws/channels/c1?key=k&dir=write',
      )
    })
  })
})
