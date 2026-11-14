import { type AddressInfo, createServer, type Socket } from 'node:net'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { WebSocket, WebSocketServer } from 'ws'
import { inputFromRef, outputFromRef } from '../src/call-streams'
import { closeReason } from '../src/channels'
import { StreamAbortedError } from '../src/errors'
import type { StreamChannelRef } from '../src/iii-types'

// A real ws server stands in for the engine's channel endpoint, so close codes,
// resets and frames behave as they do on the wire. The client is `ws`'s
// WebSocket in the place of the browser's (the same API; setup.ts restores the
// global after each test).

const ref = (direction: 'read' | 'write'): StreamChannelRef => ({
  channel_id: 'test-channel',
  access_key: 'test-key',
  direction,
})

const waitFor = async <T>(get: () => T | undefined, ms = 5000): Promise<T> => {
  const deadline = Date.now() + ms
  for (;;) {
    const v = get()
    if (v !== undefined) return v
    if (Date.now() > deadline) throw new Error('timed out waiting for condition')
    await new Promise((r) => setTimeout(r, 10))
  }
}

/** The TCP socket under a `ws` connection, reached for testing. */
const rawSocket = (ws: WebSocket): Socket => (ws as unknown as { _socket: Socket })._socket

const closeOf = (ws: WebSocket) =>
  new Promise<{ code: number; reason: string }>((resolve) =>
    ws.once('close', (code, reason) => resolve({ code, reason: reason.toString() })),
  )

const text = (bytes: Uint8Array) => new TextDecoder().decode(bytes)

describe('stream transport over a real WebSocket', () => {
  let wss: WebSocketServer
  let url: string
  let peer: WebSocket | undefined
  let peerUrl: string | undefined
  /** What the peer received (`b:` binary, `t:` text), recorded from the moment it connected. */
  let peerFrames: string[]
  let peerSizes: number[]
  /** The peer's close, recorded from the moment it connected. */
  let peerClosed: Promise<{ code: number; reason: string }>

  beforeEach(async () => {
    globalThis.WebSocket = WebSocket as unknown as typeof globalThis.WebSocket
    wss = new WebSocketServer({ host: '127.0.0.1', port: 0 })
    await new Promise<void>((resolve) => wss.once('listening', () => resolve()))
    url = `ws://127.0.0.1:${(wss.address() as { port: number }).port}`
    peer = undefined
    peerUrl = undefined
    peerFrames = []
    peerSizes = []
    wss.on('connection', (ws, req) => {
      peer = ws
      peerUrl = req.url
      peerClosed = closeOf(ws)
      ws.on('message', (data: Buffer, isBinary) => {
        peerFrames.push(`${isBinary ? 'b' : 't'}:${data.toString()}`)
        peerSizes.push(data.length)
      })
    })
  })

  afterEach(async () => {
    for (const client of wss.clients) client.terminate()
    await new Promise<void>((resolve) => wss.close(() => resolve()))
  })

  describe('InputStream on a read ref', () => {
    // Wrapped so awaiting the setup does not also await the read.
    const endWith = async (end: (ws: WebSocket) => void): Promise<{ all: Promise<Uint8Array> }> => {
      const all = inputFromRef(ref('read'), { wsBase: url }).bytes()
      all.catch(() => undefined)
      const ws = await waitFor(() => peer)
      ws.send(Buffer.from('partial'), { binary: true })
      end(ws)
      return { all }
    }

    it('opens lazily, on the first read, at the read URL of its ref', async () => {
      const input = inputFromRef(ref('read'), { wsBase: url })
      await new Promise((r) => setTimeout(r, 50))
      expect(peer).toBeUndefined()
      const all = input.bytes()
      const ws = await waitFor(() => peer)
      expect(peerUrl).toBe('/ws/channels/test-channel?key=test-key&dir=read')
      ws.close(1000)
      expect((await all).length).toBe(0)
    })

    it('rejects with STREAM_ABORTED on close 1011, with the engine reason', async () => {
      const { all } = await endWith((ws) => ws.close(1011, 'channel writer aborted: closed with code 4011'))
      const error = await all.catch((e: unknown) => e)
      expect(error).toBeInstanceOf(StreamAbortedError)
      expect(error).toMatchObject({ code: 'STREAM_ABORTED', reason: 'channel writer aborted: closed with code 4011' })
    })

    it('rejects when the connection drops without a close frame (1006)', async () => {
      const { all } = await endWith((ws) => ws.terminate())
      await expect(all).rejects.toThrow(/STREAM_ABORTED/)
    })

    // A socket that never opens must fail, not read as an empty stream.
    it('rejects with STREAM_ABORTED when the connection is refused', async () => {
      const closed = createServer()
      await new Promise<void>((resolve) => closed.listen(0, '127.0.0.1', resolve))
      const { port } = closed.address() as AddressInfo
      await new Promise((resolve) => closed.close(resolve))

      await expect(inputFromRef(ref('read'), { wsBase: `ws://127.0.0.1:${port}` }).bytes()).rejects.toThrow(
        /STREAM_ABORTED.*ECONNREFUSED/,
      )
    })

    it('rejects with STREAM_ABORTED when the upgrade is answered with 404 (an end the engine does not know)', async () => {
      const notFound = new WebSocketServer({ host: '127.0.0.1', port: 0, verifyClient: (_info, cb) => cb(false, 404) })
      await new Promise<void>((resolve) => notFound.once('listening', () => resolve()))
      try {
        const base = `ws://127.0.0.1:${(notFound.address() as AddressInfo).port}`
        await expect(inputFromRef(ref('read'), { wsBase: base }).bytes()).rejects.toThrow(
          /STREAM_ABORTED.*Unexpected server response: 404/,
        )
      } finally {
        await new Promise<void>((resolve) => notFound.close(() => resolve()))
      }
    })

    it('throws in for-await after the data sent before the abort', async () => {
      const input = inputFromRef(ref('read'), { wsBase: url })
      const chunks: Uint8Array[] = []
      const loop = (async () => {
        for await (const chunk of input) chunks.push(chunk)
      })()
      const ws = await waitFor(() => peer)
      ws.send(Buffer.from('partial'), { binary: true })
      ws.close(1001, 'going away')

      await expect(loop).rejects.toThrow(/going away/)
      expect(text(chunks[0])).toBe('partial')
    })

    const slowConsumers: Record<
      string,
      (input: ReturnType<typeof inputFromRef>, chunks: Uint8Array[]) => Promise<void>
    > = {
      'for-await': async (input, chunks) => {
        for await (const chunk of input) {
          chunks.push(chunk)
          await new Promise((r) => setTimeout(r, 20))
        }
      },
      'toWebStream()': (input, chunks) =>
        input.toWebStream().pipeTo(
          new WritableStream<Uint8Array>(
            {
              write: (chunk) =>
                new Promise<void>((resolve) => {
                  chunks.push(chunk)
                  setTimeout(resolve, 5)
                }),
            },
            { highWaterMark: 1 },
          ),
        ),
    }

    it.each(
      Object.keys(slowConsumers),
    )('a slow %s consumer gets all data sent before the abort, then the error', async (kind) => {
      const input = inputFromRef(ref('read'), { wsBase: url })
      const chunks: Uint8Array[] = []
      const consumed = slowConsumers[kind](input, chunks)
      const ws = await waitFor(() => peer)
      const sent = Array.from({ length: 20 }, (_, i) => Buffer.alloc(1024, i))
      for (const chunk of sent) ws.send(chunk, { binary: true })
      ws.close(1011, 'channel writer aborted: closed with code 4011')

      await expect(consumed).rejects.toThrow(/STREAM_ABORTED/)
      expect(Buffer.concat(chunks)).toEqual(Buffer.concat(sent))
    })

    it('ends cleanly on close 1000', async () => {
      const { all } = await endWith((ws) => ws.close(1000))
      expect(text(await all)).toBe('partial')
    })

    it('ends cleanly on a close frame with no code (1005)', async () => {
      const { all } = await endWith((ws) => ws.close())
      expect(text(await all)).toBe('partial')
    })

    it('keeps text and binary frames apart, in order', async () => {
      const frames = (async () => {
        const out: string[] = []
        for await (const f of inputFromRef(ref('read'), { wsBase: url }).frames()) {
          out.push(f.type === 'text' ? `t:${f.value}` : `b:${text(f.value)}`)
        }
        return out
      })()
      const ws = await waitFor(() => peer)
      ws.send('{"n":1}')
      ws.send(Buffer.from('a'), { binary: true })
      ws.send('not json')
      ws.send(Buffer.from('b'), { binary: true })
      ws.close(1000)
      expect(await frames).toEqual(['t:{"n":1}', 'b:a', 't:not json', 'b:b'])
    })

    it('messages() parses JSON text frames and yields others as strings; bytes are skipped', async () => {
      const messages = (async () => {
        const out: unknown[] = []
        for await (const m of inputFromRef(ref('read'), { wsBase: url }).messages()) out.push(m)
        return out
      })()
      const ws = await waitFor(() => peer)
      ws.send('{"type":"progress","n":1}')
      ws.send(Buffer.from('bytes'), { binary: true })
      ws.send('plain')
      ws.close(1000)
      expect(await messages).toEqual([{ type: 'progress', n: 1 }, 'plain'])
    })

    it('cancel() closes its socket cleanly, which tells the engine the reader left', async () => {
      const input = inputFromRef(ref('read'), { wsBase: url })
      const it = input[Symbol.asyncIterator]()
      const first = it.next()
      const ws = await waitFor(() => peer)
      ws.send(Buffer.from('one'), { binary: true })
      expect(text((await first).value as Uint8Array)).toBe('one')
      const closed = closeOf(ws)
      await input.cancel()
      expect(await closed).toEqual({ code: 1000, reason: 'channel_close' })
    })

    it('cancel() on a stream never read opens it, so the writer learns the reader left', async () => {
      const input = inputFromRef(ref('read'), { wsBase: url })
      const cancelled = input.cancel()
      await waitFor(() => peer)
      expect(await peerClosed).toMatchObject({ code: 1000 })
      await cancelled
    })

    it('treats its own close as a cancel, not an abort, even when the peer drops without replying', async () => {
      const input = inputFromRef(ref('read'), { wsBase: url })
      const it = input[Symbol.asyncIterator]()
      const first = it.next()
      const ws = await waitFor(() => peer)
      // Like the engine: the reader's close frame makes it drop the socket without a reply (1006).
      rawSocket(ws).prependListener('data', () => rawSocket(ws).destroy())
      ws.send(Buffer.from('one'), { binary: true })
      await first
      await input.cancel()
      // Cancelled, not aborted (the 1006 that follows is its own close): the next read fails with STREAM_CLOSED.
      await expect(it.next()).rejects.toMatchObject({
        code: 'STREAM_CLOSED',
        message: 'STREAM_CLOSED: the stream was cancelled',
      })
    })
  })

  describe('OutputStream on a write ref', () => {
    it('opens on its first write, at the write URL of its ref, and closes with 1000 after the data', async () => {
      const out = outputFromRef(ref('write'), { wsBase: url })
      await new Promise((r) => setTimeout(r, 50))
      expect(peer).toBeUndefined()
      const written = out.write('hello')
      await waitFor(() => peer)
      expect(peerUrl).toBe('/ws/channels/test-channel?key=test-key&dir=write')
      await written
      await out.send({ type: 'done' })
      await out.close()
      expect(await peerClosed).toEqual({ code: 1000, reason: 'stream_complete' })
      expect(peerFrames).toEqual(['b:hello', 't:{"type":"done"}'])
    })

    it('close() on a stream never used opens it and closes it cleanly (an empty stream)', async () => {
      const out = outputFromRef(ref('write'), { wsBase: url })
      const closing = out.close()
      await waitFor(() => peer)
      expect(await peerClosed).toEqual({ code: 1000, reason: 'stream_complete' })
      await closing
    })

    it('abort() closes with 4011 and the reason (cut to 123 bytes), opening the socket first (R7)', async () => {
      const out = outputFromRef(ref('write'), { wsBase: url })
      const long = 'é'.repeat(100)
      const aborting = out.abort(long)
      await waitFor(() => peer)
      const closed = await peerClosed
      expect(closed.code).toBe(4011)
      expect(closed.reason).toBe(closeReason(long))
      expect(Buffer.byteLength(closed.reason)).toBeLessThanOrEqual(123)
      await aborting
    })

    it('close() and abort() are idempotent; writes after them fail with STREAM_CLOSED', async () => {
      const out = outputFromRef(ref('write'), { wsBase: url })
      const first = out.close()
      expect(out.close()).toBe(first)
      expect(out.abort('late')).toBe(first)
      await waitFor(() => peer)
      await first
      await expect(out.write('x')).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
      await expect(out.send({})).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
    })

    it('a write fails with STREAM_CLOSED once the engine closed the socket (the reader left)', async () => {
      const out = outputFromRef(ref('write'), { wsBase: url })
      await out.write('first').catch(() => undefined)
      const ws = await waitFor(() => peer)
      ws.close()
      await waitFor(() => (ws.readyState === WebSocket.CLOSED ? true : undefined))
      await new Promise((r) => setTimeout(r, 20))
      await expect(out.write('second')).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
    })

    it('binary data goes out in frames of at most 64 KiB', async () => {
      const out = outputFromRef(ref('write'), { wsBase: url })
      const written = out.write(new Uint8Array(200 * 1024).fill(1))
      await waitFor(() => peer)
      await written
      await out.close()
      await peerClosed
      expect(peerSizes).toEqual([65536, 65536, 65536, 8192])
    })

    it('fails its writes with STREAM_CLOSED when it cannot connect', async () => {
      const closed = createServer()
      await new Promise<void>((resolve) => closed.listen(0, '127.0.0.1', resolve))
      const { port } = closed.address() as AddressInfo
      await new Promise((resolve) => closed.close(resolve))

      const out = outputFromRef(ref('write'), { wsBase: `ws://127.0.0.1:${port}` })
      await expect(out.write('hi')).rejects.toMatchObject({
        code: 'STREAM_CLOSED',
        message: expect.stringMatching(/ECONNREFUSED/),
      })
      await expect(out.write('again')).rejects.toMatchObject({ code: 'STREAM_CLOSED' })
    })
  })
})

describe('closeReason', () => {
  it('keeps a short reason and cuts a long one on a UTF-8 boundary', () => {
    expect(closeReason('short')).toBe('short')
    const cut = closeReason('€'.repeat(60))
    expect(Buffer.byteLength(cut)).toBeLessThanOrEqual(123)
    expect(cut).toBe('€'.repeat(41))
  })
})
