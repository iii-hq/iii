import { createServer } from 'node:http'
import type { AddressInfo } from 'node:net'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { WebSocketServer } from 'ws'
import { registerWorker } from '../src/iii'
import { MessageType } from '../src/iii-types'

// Outbound JSON envelope: 64 MiB (67,108,864 bytes).
const LIMIT = 64 * 1024 * 1024
const FRAME_BYTES = 16 * 1024 * 1024
const FRAGMENT_BYTES = 1024 * 1024
// Inspect internal queue/transport state without exporting a new SDK API.
type Internal = {
  prepareJson: (message: Record<string, unknown>) => string
  sendMessage: (type: MessageType, message: Record<string, unknown>) => void
  sendMessageRaw: (data: string) => void
  call: (request: Record<string, unknown>) => Promise<unknown>
  registerFunction: (id: string, handler: (input: unknown) => Promise<unknown>) => void
  onMessage: (data: string) => void
  shutdown: () => Promise<void>
  messagesToSend: Record<string, unknown>[]
  invocations: Map<string, unknown>
  onSocketOpen: () => void
  registerWorkerMetadata: () => void
  ws: unknown
}
let sdk: Internal
function makeSdk() {
  sdk = registerWorker('ws://127.0.0.1:0', {
    enableMetricsReporting: false,
    reconnectionConfig: { maxRetries: 0 },
  }) as unknown as Internal
  return sdk
}
afterEach(async () => {
  await sdk.shutdown()
})

function envelopeAtSize(size: number) {
  const message = { type: MessageType.InvokeFunction, data: '' }
  message.data = 'x'.repeat(size - Buffer.byteLength(JSON.stringify(message)))
  return message
}

type ParsedFrame = { fin: boolean; opcode: number; masked: boolean; payload: Buffer }
function parseClientFrames(wire: Buffer): ParsedFrame[] {
  const frames: ParsedFrame[] = []
  let offset = 0
  while (offset < wire.length) {
    if (offset + 2 > wire.length) throw new Error('truncated WebSocket frame header')
    const first = wire[offset++]
    const second = wire[offset++]
    const fin = (first & 0x80) !== 0
    const opcode = first & 0x0f
    const masked = (second & 0x80) !== 0
    let length = second & 0x7f
    if (length === 126) {
      if (offset + 2 > wire.length) throw new Error('truncated 16-bit frame length')
      length = wire.readUInt16BE(offset)
      offset += 2
    } else if (length === 127) {
      if (offset + 8 > wire.length) throw new Error('truncated 64-bit frame length')
      const longLength = wire.readBigUInt64BE(offset)
      if (longLength > BigInt(Number.MAX_SAFE_INTEGER)) throw new Error('frame is too large to parse')
      length = Number(longLength)
      offset += 8
    }
    const mask = masked ? wire.subarray(offset, offset + 4) : undefined
    if (masked) offset += 4
    if (offset + length > wire.length) throw new Error('truncated WebSocket frame payload')
    const payload = Buffer.from(wire.subarray(offset, offset + length))
    offset += length
    if (mask) {
      for (let index = 0; index < payload.length; index++) payload[index] ^= mask[index % 4]
    }
    frames.push({ fin, opcode, masked, payload })
  }
  return frames
}

describe('JSON frame size', () => {
  it.each([-1, 0, 1])('uses the inclusive complete UTF-8 envelope boundary (%i)', (delta) => {
    makeSdk()
    const message = envelopeAtSize(LIMIT + delta)
    if (delta > 0) {
      expect(() => sdk.prepareJson(message)).toThrow(
        `Serialized JSON envelope is ${LIMIT + delta} bytes; limit is ${LIMIT} bytes. Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size`,
      )
    } else expect(Buffer.byteLength(sdk.prepareJson(message))).toBe(LIMIT + delta)
  })

  it('counts multibyte Unicode and serialization escapes by UTF-8 bytes', () => {
    makeSdk()
    const pattern = `${String.fromCodePoint(0x1f600)}${String.fromCharCode(0xe9, 10, 34, 92)}`
    const accepted = { type: MessageType.InvokeFunction, data: pattern.repeat(1700000) }
    const acceptedJson = JSON.stringify(accepted)
    expect(accepted.data.length).toBeLessThan(LIMIT)
    expect(Buffer.byteLength(acceptedJson)).toBeLessThan(LIMIT)
    expect(sdk.prepareJson(accepted)).toBe(acceptedJson)

    const rejected = { type: MessageType.InvokeFunction, data: pattern.repeat(6_000_000) }
    const rejectedJson = JSON.stringify(rejected)
    const size = Buffer.byteLength(rejectedJson, 'utf8')
    expect(rejected.data.length).toBeLessThan(LIMIT)
    expect(rejectedJson.length).toBeLessThan(LIMIT)
    expect(size).toBeGreaterThan(LIMIT)
    expect(() => sdk.prepareJson(rejected)).toThrow(
      `Serialized JSON envelope is ${size} bytes; limit is ${LIMIT} bytes. Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size`,
    )
  })

  it('accepts 16 MiB plus one byte and approximately 40 MiB', () => {
    makeSdk()
    expect(Buffer.byteLength(sdk.prepareJson(envelopeAtSize(FRAME_BYTES + 1)))).toBe(FRAME_BYTES + 1)
    expect(Buffer.byteLength(sdk.prepareJson(envelopeAtSize(40 * 1024 * 1024)))).toBe(40 * 1024 * 1024)
  })

  it('substitutes queued results and rejects queued arguments without pending entries', async () => {
    makeSdk()
    await expect(sdk.call({ function_id: 'f', payload: 'x'.repeat(LIMIT) })).rejects.toMatchObject({
      code: 'payload_too_large',
    })
    expect(sdk.invocations.size).toBe(0)
    expect(sdk.messagesToSend).toHaveLength(0)
    sdk.sendMessage(MessageType.InvocationResult, { invocation_id: 'i', function_id: 'f', result: 'x'.repeat(LIMIT) })
    expect(sdk.messagesToSend[0]).toMatchObject({ invocation_id: 'i', error: { code: 'payload_too_large' } })
    expect(sdk.messagesToSend[0]).not.toHaveProperty('result')
  })

  it('sends an oversized result as a bare error message, so the caller sees the code prefix once', async () => {
    makeSdk()
    sdk.registerFunction('size::big', async () => 'x'.repeat(LIMIT))
    sdk.onMessage(
      JSON.stringify({
        type: MessageType.InvokeFunction,
        invocation_id: 'target',
        function_id: 'size::big',
        data: null,
      }),
    )
    const frame = await vi.waitFor(() => {
      const sent = sdk.messagesToSend.find((message) => message.invocation_id === 'target')
      if (!sent) throw new Error('result not sent yet')
      return sent as { type: string; function_id: string; error: { code: string; message: string } }
    })
    expect(frame).toMatchObject({ type: MessageType.InvocationResult, function_id: 'size::big' })
    expect(frame).not.toHaveProperty('result')
    expect(frame.error.code).toBe('payload_too_large')
    const match =
      /^Serialized JSON envelope is (\d+) bytes; limit is 67108864 bytes\. Use channels for larger payloads: https:\/\/iii\.dev\/docs\/creating-workers\/channels#payload-size$/.exec(
        frame.error.message,
      )
    expect(match, frame.error.message).not.toBeNull()
    expect(Number(match?.[1])).toBeGreaterThan(LIMIT)

    // The engine relays the target's error body to the caller unchanged.
    const call = sdk.call({ function_id: 'size::remote', payload: null })
    const invocation = sdk.messagesToSend.find((message) => message.function_id === 'size::remote')
    expect(invocation?.invocation_id).toEqual(expect.any(String))
    sdk.onMessage(JSON.stringify({ ...frame, invocation_id: invocation?.invocation_id, function_id: 'size::remote' }))
    const rejection = (await call.catch((error) => error)) as Error & { code?: string }
    expect(rejection.code).toBe('payload_too_large')
    expect(rejection.message).toBe(`payload_too_large: ${frame.error.message}`)
    expect(rejection.message.match(/payload_too_large: /g)).toHaveLength(1)
  })

  it('writes masked text and continuation frames <= 1 MiB without interleaving and preserves UTF-8', async () => {
    const httpServer = createServer()
    const webSocketServer = new WebSocketServer({ noServer: true })
    httpServer.on('upgrade', (request, socket, head) => {
      webSocketServer.handleUpgrade(request, socket, head, (client) => {
        webSocketServer.emit('connection', client, request)
      })
    })
    const received: Array<{ data: Buffer; isBinary: boolean }> = []
    let resolveConnected: (() => void) | undefined
    let resolveMessages: (() => void) | undefined
    const serverSawMetadata = new Promise<void>((resolve) => {
      resolveConnected = resolve
    })
    const allMessages = new Promise<void>((resolve) => {
      resolveMessages = resolve
    })
    webSocketServer.on('connection', (client) => {
      client.on('message', (data, isBinary) => {
        const bytes = Buffer.from(data as Buffer)
        if (received.length === 0) resolveConnected?.()
        received.push({ data: bytes, isBinary })
        if (received.length === 3) resolveMessages?.()
      })
    })
    await new Promise<void>((resolve) => httpServer.listen(0, '127.0.0.1', resolve))
    const port = (httpServer.address() as AddressInfo).port
    sdk = registerWorker(`ws://127.0.0.1:${port}`, {
      enableMetricsReporting: false,
      otel: { enabled: false },
      reconnectionConfig: { maxRetries: 0 },
    }) as unknown as Internal
    try {
      await serverSawMetadata
      received.length = 0
      const transport = Reflect.get(sdk.ws as object, '_socket') as object
      const originalWrite = Reflect.get(transport, 'write') as (...args: unknown[]) => boolean
      const wireWrites: Buffer[] = []
      Reflect.set(transport, 'write', (...args: unknown[]) => {
        const chunk = args[0]
        if (typeof chunk === 'string' || chunk instanceof Uint8Array) wireWrites.push(Buffer.from(chunk))
        return Reflect.apply(originalWrite, transport, args)
      })

      const singleFrame = { type: MessageType.InvokeFunction, invocation_id: 'single', function_id: 'f', data: '' }
      singleFrame.data = 's'.repeat(FRAME_BYTES - Buffer.byteLength(JSON.stringify(singleFrame)))
      const singleFrameJson = JSON.stringify(singleFrame)
      expect(Buffer.byteLength(singleFrameJson)).toBe(FRAME_BYTES)
      sdk.sendMessageRaw(singleFrameJson)

      const targetSize = FRAME_BYTES + 1
      const fragmented = { type: MessageType.InvokeFunction, invocation_id: 'fragmented', function_id: 'f', data: '' }
      const overhead = Buffer.byteLength(JSON.stringify(fragmented))
      const prefixLength = FRAGMENT_BYTES - overhead - 1
      fragmented.data = `${'x'.repeat(prefixLength)}😀${'y'.repeat(targetSize - overhead - prefixLength - 4)}`
      const largeEnvelope = JSON.stringify(fragmented)
      expect(Buffer.byteLength(largeEnvelope)).toBe(targetSize)
      sdk.sendMessageRaw(largeEnvelope)
      sdk.sendMessageRaw(
        JSON.stringify({ type: MessageType.InvocationResult, invocation_id: 'after', function_id: 'f', result: 'ok' }),
      )
      await allMessages

      const frames = parseClientFrames(Buffer.concat(wireWrites))
      expect(frames[0]).toMatchObject({ fin: true, opcode: 1, masked: true })
      expect(frames[0].payload.length).toBe(FRAME_BYTES)
      const fragmentCount = Math.ceil(targetSize / FRAGMENT_BYTES)
      const fragmentStart = 1
      const fragments = frames.slice(fragmentStart, fragmentStart + fragmentCount)
      expect(fragments).toHaveLength(fragmentCount)
      expect(fragments.map((frame) => frame.opcode)).toEqual([1, ...Array(fragmentCount - 1).fill(0)])
      expect(fragments.slice(0, -1).every((frame) => !frame.fin)).toBe(true)
      expect(fragments[fragmentCount - 1].fin).toBe(true)
      expect(fragments.every((frame) => frame.masked && frame.payload.length <= FRAGMENT_BYTES)).toBe(true)
      expect(frames[fragmentStart + fragmentCount]).toMatchObject({ opcode: 1, fin: true, masked: true })
      expect(JSON.parse(received[0].data.toString('utf8'))).toEqual(singleFrame)
      expect(received[0].isBinary).toBe(false)
      expect(JSON.parse(received[1].data.toString('utf8'))).toEqual(fragmented)
      expect(received[1].isBinary).toBe(false)
      expect(JSON.parse(received[2].data.toString('utf8'))).toMatchObject({ invocation_id: 'after', result: 'ok' })
      expect(received[2].isBinary).toBe(false)
    } finally {
      await sdk.shutdown()
      await new Promise<void>((resolve) => webSocketServer.close(() => resolve()))
      await new Promise<void>((resolve) => httpServer.close(() => resolve()))
    }
  })

  it('terminates the socket when a synchronous fragment send throws after the first frame', () => {
    makeSdk()
    const original = sdk.ws as {
      removeAllListeners: () => void
      on: (event: string, handler: () => void) => void
      terminate: () => void
    }
    original.removeAllListeners()
    original.on('error', () => {})
    original.terminate()
    let sends = 0
    const socket = {
      readyState: 1,
      send: vi.fn(() => {
        sends += 1
        if (sends === 2) throw new Error('synchronous send failure')
      }),
      close: vi.fn(),
      removeAllListeners: vi.fn(),
      on: vi.fn(),
      terminate: vi.fn(),
      ping: vi.fn(),
    }
    sdk.ws = socket
    const message = { type: MessageType.InvokeFunction, data: 'x'.repeat(FRAME_BYTES + 1) }
    sdk.sendMessageRaw(JSON.stringify(message))
    expect(sends).toBe(2)
    expect(socket.terminate).toHaveBeenCalledOnce()
  })

  it('guards direct raw sends and keeps the same socket for a subsequent small result', () => {
    makeSdk()
    const send = vi.fn((_data, callback) => callback?.())
    const original = sdk.ws as {
      removeAllListeners: () => void
      on: (event: string, handler: () => void) => void
      terminate: () => void
    }
    original.removeAllListeners()
    original.on('error', () => {})
    original.terminate()
    const socket = {
      readyState: 1,
      send,
      close: vi.fn(),
      removeAllListeners: vi.fn(),
      on: vi.fn(),
      terminate: vi.fn(),
      ping: vi.fn(),
    }
    sdk.ws = socket
    sdk.registerWorkerMetadata = vi.fn()
    sdk.messagesToSend = [
      { type: MessageType.InvocationResult, invocation_id: 'queued', function_id: 'f', result: 'x'.repeat(LIMIT) },
      { type: MessageType.InvokeFunction, function_id: 'f', data: {} },
    ]
    sdk.onSocketOpen()
    expect(sdk.messagesToSend).toHaveLength(0)
    expect(JSON.parse(send.mock.calls[0][0])).toMatchObject({
      invocation_id: 'queued',
      error: { code: 'payload_too_large' },
    })
    send.mockClear()
    sdk.sendMessageRaw(
      JSON.stringify({
        type: MessageType.InvocationResult,
        invocation_id: 'i',
        function_id: 'f',
        result: 'x'.repeat(LIMIT),
      }),
    )
    expect(JSON.parse(send.mock.calls[0][0])).toMatchObject({
      invocation_id: 'i',
      error: { code: 'payload_too_large' },
    })
    sdk.sendMessage(MessageType.InvocationResult, { invocation_id: 'j', function_id: 'f', result: 1 })
    expect(sdk.ws).toBe(socket)
    expect(send).toHaveBeenCalledTimes(2)
    expect(socket.close).not.toHaveBeenCalled()
  })
})
