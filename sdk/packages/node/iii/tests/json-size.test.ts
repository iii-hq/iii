import { afterEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../src/iii'
import { MessageType } from '../src/iii-types'

const LIMIT = 16 * 1024 * 1024
// Inspect internal queue/transport state without exporting a new SDK API.
type Internal = {
  prepareJson: (message: Record<string, unknown>) => string
  sendMessage: (type: MessageType, message: Record<string, unknown>) => void
  sendMessageRaw: (data: string) => void
  trigger: (request: Record<string, unknown>) => Promise<unknown>
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
afterEach(async () => { await sdk.shutdown() })

describe('MOT-4988 JSON frame size', () => {
  it.each([-1, 0, 1])('uses the inclusive complete UTF-8 envelope boundary (%i)', (delta) => {
    makeSdk()
    const message = { type: MessageType.InvokeFunction, data: '' }
    message.data = 'x'.repeat(LIMIT - Buffer.byteLength(JSON.stringify(message)) + delta)
    if (delta > 0) expect(() => sdk.prepareJson(message)).toThrow('payload_too_large')
    else expect(Buffer.byteLength(sdk.prepareJson(message))).toBe(LIMIT + delta)
  })

  it('counts multibyte Unicode and serialization escapes', () => {
    makeSdk()
    const message = { type: MessageType.InvokeFunction, data: '😀é\n"\\'.repeat(1700000) }
    expect(message.data.length).toBeLessThan(LIMIT)
    expect(Buffer.byteLength(JSON.stringify(message))).toBeGreaterThan(LIMIT)
    expect(() => sdk.prepareJson(message)).toThrow('payload_too_large')
  })

  it('substitutes queued results and rejects queued arguments without pending entries', async () => {
    makeSdk()
    await expect(sdk.trigger({ function_id: 'f', payload: 'x'.repeat(LIMIT) })).rejects.toMatchObject({ code: 'payload_too_large' })
    expect(sdk.invocations.size).toBe(0)
    expect(sdk.messagesToSend).toHaveLength(0)
    sdk.sendMessage(MessageType.InvocationResult, { invocation_id: 'i', function_id: 'f', result: 'x'.repeat(LIMIT) })
    expect(sdk.messagesToSend[0]).toMatchObject({ invocation_id: 'i', error: { code: 'payload_too_large' } })
    expect(sdk.messagesToSend[0]).not.toHaveProperty('result')
  })

  it('guards direct raw sends and keeps the same socket for a subsequent small result', () => {
    makeSdk()
    const send = vi.fn((_data, callback) => callback?.())
    const original = sdk.ws as { removeAllListeners: () => void; on: (event: string, handler: () => void) => void; terminate: () => void }
    original.removeAllListeners()
    original.on('error', () => {})
    original.terminate()
    const socket = { readyState: 1, send, close: vi.fn(), removeAllListeners: vi.fn(), on: vi.fn(), terminate: vi.fn(), ping: vi.fn() }
    sdk.ws = socket
    sdk.registerWorkerMetadata = vi.fn()
    sdk.messagesToSend = [
      { type: MessageType.InvocationResult, invocation_id: 'queued', function_id: 'f', result: 'x'.repeat(LIMIT) },
      { type: MessageType.InvokeFunction, function_id: 'f', data: {} },
    ]
    sdk.onSocketOpen()
    expect(sdk.messagesToSend).toHaveLength(0)
    expect(JSON.parse(send.mock.calls[0][0])).toMatchObject({ invocation_id: 'queued', error: { code: 'payload_too_large' } })
    send.mockClear()
    sdk.sendMessageRaw(JSON.stringify({ type: MessageType.InvocationResult, invocation_id: 'i', function_id: 'f', result: 'x'.repeat(LIMIT) }))
    expect(JSON.parse(send.mock.calls[0][0])).toMatchObject({ invocation_id: 'i', error: { code: 'payload_too_large' } })
    sdk.sendMessage(MessageType.InvocationResult, { invocation_id: 'j', function_id: 'f', result: 1 })
    expect(sdk.ws).toBe(socket)
    expect(send).toHaveBeenCalledTimes(2)
    expect(socket.close).not.toHaveBeenCalled()
  })
})
