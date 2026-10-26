import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { type AddressInfo, createServer, type Socket } from 'node:net'
import { Writable } from 'node:stream'
import { pipeline } from 'node:stream/promises'
import { WebSocket, WebSocketServer } from 'ws'
import { ChannelReader, ChannelWriter } from '../src/channels'
import type { StreamChannelRef } from '../src/iii-types'

// A real ws server stands in for the engine, so close codes, resets and
// socket buffering behave as they do on the wire.

const ref = (direction: 'read' | 'write'): StreamChannelRef => ({
  channel_id: 'test-channel',
  access_key: 'test-key',
  direction,
})

const waitFor = async <T,>(get: () => T | undefined, ms = 5000): Promise<T> => {
  const deadline = Date.now() + ms
  for (;;) {
    const v = get()
    if (v !== undefined) return v
    if (Date.now() > deadline) throw new Error('timed out waiting for condition')
    await new Promise((r) => setTimeout(r, 10))
  }
}

// biome-ignore lint/suspicious/noExplicitAny: reaching the ws raw socket for testing
const rawSocket = (ws: WebSocket): Socket => (ws as any)._socket

describe('channel abort over a real WebSocket', () => {
  let wss: WebSocketServer
  let url: string
  let peer: WebSocket | undefined

  beforeEach(async () => {
    // Bind the address the client dials: a wildcard bind can share its port with another 127.0.0.1 listener on macOS.
    wss = new WebSocketServer({ host: '127.0.0.1', port: 0 })
    await new Promise<void>((resolve) => wss.once('listening', () => resolve()))
    url = `ws://127.0.0.1:${(wss.address() as { port: number }).port}`
    peer = undefined
    wss.on('connection', (ws) => {
      peer = ws
    })
  })

  afterEach(async () => {
    for (const client of wss.clients) client.terminate()
    await new Promise<void>((resolve) => wss.close(() => resolve()))
  })

  describe('reader', () => {
    // Wrapped so awaiting the setup does not also await readAll.
    const endWith = async (end: (ws: WebSocket) => void): Promise<{ all: Promise<Buffer> }> => {
      const reader = new ChannelReader(url, ref('read'))
      const all = reader.readAll()
      all.catch(() => {})
      const ws = await waitFor(() => peer)
      ws.send(Buffer.from('partial'), { binary: true })
      end(ws)
      return { all }
    }

    it('rejects readAll on close 1011 with the code and reason', async () => {
      const { all } = await endWith((ws) => ws.close(1011, 'channel writer aborted: connection lost before close'))
      await expect(all).rejects.toThrow(/1011.*channel writer aborted: connection lost before close/)
    })

    it('rejects readAll when the connection drops without a close frame (1006)', async () => {
      const { all } = await endWith((ws) => ws.terminate())
      await expect(all).rejects.toThrow(/1006/)
    })

    // A socket that never opens must fail with its own error, not read as an empty stream or a generic 1006.
    it('rejects readAll with the socket error when the connection is refused', async () => {
      const closed = createServer()
      await new Promise<void>((resolve) => closed.listen(0, '127.0.0.1', resolve))
      const { port } = closed.address() as AddressInfo
      await new Promise((resolve) => closed.close(resolve))

      await expect(new ChannelReader(`ws://127.0.0.1:${port}`, ref('read')).readAll()).rejects.toThrow(/ECONNREFUSED/)
    })

    it('rejects readAll with the socket error when the upgrade is answered with 404', async () => {
      const notFound = new WebSocketServer({ host: '127.0.0.1', port: 0, verifyClient: (_info, cb) => cb(false, 404) })
      await new Promise<void>((resolve) => notFound.once('listening', () => resolve()))
      try {
        const reader = new ChannelReader(`ws://127.0.0.1:${(notFound.address() as AddressInfo).port}`, ref('read'))
        await expect(reader.readAll()).rejects.toThrow(/Unexpected server response: 404/)
      } finally {
        await new Promise<void>((resolve) => notFound.close(() => resolve()))
      }
    })

    it('throws in for-await after the data sent before the abort', async () => {
      const reader = new ChannelReader(url, ref('read'))
      const chunks: Buffer[] = []
      const loop = (async () => {
        for await (const chunk of reader.stream) chunks.push(chunk)
      })()
      const ws = await waitFor(() => peer)
      ws.send(Buffer.from('partial'), { binary: true })
      ws.close(1001, 'going away')

      await expect(loop).rejects.toThrow(/1001.*going away/)
      expect(Buffer.concat(chunks).toString()).toBe('partial')
    })

    // A pipe pauses with a read still pending, so the abort must re-arm read() to surface once the buffer drains.
    const slowConsumers: Record<string, (stream: NodeJS.ReadableStream, chunks: Buffer[]) => Promise<void>> = {
      'for-await': async (stream, chunks) => {
        for await (const chunk of stream) {
          chunks.push(chunk as Buffer)
          await new Promise((r) => setTimeout(r, 20))
        }
      },
      pipe: (stream, chunks) =>
        pipeline(
          stream,
          new Writable({
            highWaterMark: 1,
            write: (chunk, _enc, cb) => {
              chunks.push(chunk)
              setTimeout(cb, 5)
            },
          }),
        ),
    }

    it.each(Object.keys(slowConsumers))(
      'a slow %s consumer gets all data sent before the abort, then the error',
      async (kind) => {
        const reader = new ChannelReader(url, ref('read'))
        const chunks: Buffer[] = []
        const consumed = slowConsumers[kind](reader.stream, chunks)
        const ws = await waitFor(() => peer)
        const sent = Array.from({ length: 20 }, (_, i) => Buffer.alloc(1024, i))
        for (const chunk of sent) ws.send(chunk, { binary: true })
        ws.close(1011, 'channel writer aborted: connection lost before close')

        await expect(consumed).rejects.toThrow(/1011/)
        expect(Buffer.concat(chunks)).toEqual(Buffer.concat(sent))
      },
    )

    it('ends cleanly on close 1000', async () => {
      const { all } = await endWith((ws) => ws.close(1000))
      expect((await all).toString()).toBe('partial')
    })

    it('ends cleanly on a close frame with no code (1005)', async () => {
      const { all } = await endWith((ws) => ws.close())
      expect((await all).toString()).toBe('partial')
    })

    it('treats its own close as end of stream even when the peer drops without replying', async () => {
      const reader = new ChannelReader(url, ref('read'))
      const all = reader.readAll()
      const ws = await waitFor(() => peer)
      // Like the engine: the reader's close frame makes it drop the socket without a reply (1006).
      rawSocket(ws).prependListener('data', () => rawSocket(ws).destroy())
      // biome-ignore lint/suspicious/noExplicitAny: waiting on the private socket
      await waitFor(() => ((reader as any).ws?.readyState === WebSocket.OPEN ? true : undefined))
      reader.close()

      expect((await all).length).toBe(0)
    })

    it('treats its own close before the socket opened as end of stream', async () => {
      const reader = new ChannelReader(url, ref('read'))
      const all = reader.readAll()
      reader.close()

      expect((await all).length).toBe(0)
    })
  })

  describe('writer', () => {
    const closeOf = (ws: WebSocket) =>
      new Promise<{ code: number; reason: string }>((resolve) =>
        ws.once('close', (code, reason) => resolve({ code, reason: reason.toString() })),
      )

    it('end() under backpressure still puts its close(1000) frame on the wire', async () => {
      const writer = new ChannelWriter(url, ref('write'))
      writer.sendMessage('first')
      const ws = await waitFor(() => peer)
      ws.pause()
      const big = 'x'.repeat(1024 * 1024)
      let received = 0
      ws.on('message', (data: Buffer) => {
        if (data.length === big.length) received++
      })
      const closed = closeOf(ws)
      // biome-ignore lint/suspicious/noExplicitAny: reading the private socket
      const clientWs: WebSocket = (writer as any).ws
      await waitFor(() => (clientWs.readyState === WebSocket.OPEN ? true : undefined))

      // Fill the kernel buffers until bytes stay queued in user space, where the close frame will wait too.
      let sent = 0
      while (sent < 256) {
        writer.sendMessage(big)
        sent++
        await new Promise((r) => setTimeout(r, 5))
        if (clientWs.bufferedAmount > 0) break
      }
      expect(clientWs.bufferedAmount).toBeGreaterThan(0)

      await new Promise<void>((resolve) => writer.stream.end(resolve))
      // Let autoDestroy run before the peer drains the backlog.
      await new Promise((r) => setTimeout(r, 50))
      ws.resume()

      expect(await closed).toEqual({ code: 1000, reason: 'stream_complete' })
      expect(received).toBe(sent)
    })

    it("end() keeps its close frame past ws's 30 s close timeout while the engine is not reading", async () => {
      const writer = new ChannelWriter(url, ref('write'))
      writer.sendMessage('first')
      const ws = await waitFor(() => peer)
      ws.pause()
      const closed = closeOf(ws)
      // biome-ignore lint/suspicious/noExplicitAny: reading the private socket
      const clientWs: WebSocket = (writer as any).ws
      await waitFor(() => (clientWs.readyState === WebSocket.OPEN ? true : undefined))
      while (clientWs.bufferedAmount === 0) {
        writer.sendMessage('x'.repeat(1024 * 1024))
        await new Promise((r) => setTimeout(r, 5))
      }

      // Fake only the timers, so ws's close timer can run out without the test waiting for it.
      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
      try {
        const ended = new Promise<void>((resolve) => writer.stream.end(resolve))
        await vi.advanceTimersByTimeAsync(31_000)
        await ended
      } finally {
        vi.useRealTimers()
      }
      ws.resume()

      expect(await closed).toEqual({ code: 1000, reason: 'stream_complete' })
    })

    it('destroy(err) drops the socket without a close frame', async () => {
      const writer = new ChannelWriter(url, ref('write'))
      writer.stream.on('error', () => {})
      await new Promise((r) => writer.stream.write(Buffer.from('partial'), r))
      const closed = closeOf(await waitFor(() => peer))
      writer.stream.destroy(new Error('boom'))

      expect((await closed).code).toBe(1006)
    })

    it('destroy() before end drops the socket without a close frame', async () => {
      const writer = new ChannelWriter(url, ref('write'))
      await new Promise((r) => writer.stream.write(Buffer.from('partial'), r))
      const closed = closeOf(await waitFor(() => peer))
      writer.stream.destroy()

      expect((await closed).code).toBe(1006)
    })
  })
})
