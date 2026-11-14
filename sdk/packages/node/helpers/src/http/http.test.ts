import { Writable } from 'node:stream'
import { describe, expect, it } from 'vitest'
import { http } from './index'

/** A stand-in for the SDK's OutputStream that records what the response sent. */
function fakeResponse() {
  const sent: unknown[] = []
  let toWritableCalls = 0
  const response = {
    write: async (chunk: Uint8Array | string) => {
      sent.push({ write: Buffer.from(chunk).toString() })
    },
    send: async (message: unknown) => {
      sent.push({ send: message })
    },
    close: async () => {
      sent.push('close')
    },
    abort: async () => {
      sent.push('abort')
    },
    keepOpen() {
      return this
    },
    toWritable: () => {
      toWritableCalls++
      return new Writable({
        write(chunk, _encoding, callback) {
          sent.push({ write: Buffer.from(chunk).toString() })
          callback()
        },
        final(callback) {
          sent.push('close')
          callback()
        },
      })
    },
    toWebStream: () => new WritableStream<Uint8Array>(),
  }
  return { response, sent, toWritableCalls: () => toWritableCalls }
}

const request = {
  path_params: {},
  query_params: {},
  body: { a: 1 },
  headers: {},
  method: 'POST',
  // biome-ignore lint/suspicious/noExplicitAny: the request body is not read here
  request_body: {} as any,
}

describe('http()', () => {
  it('sends the status and headers as set_status and set_headers messages, then the body', async () => {
    const { response, sent } = fakeResponse()
    const handler = http(async (_req, res) => {
      await res.status(201)
      await res.headers({ 'content-type': 'text/plain' })
      await res.write('hello')
      await res.close()
    })
    await handler({ ...request, response })
    expect(sent).toEqual([
      { send: { type: 'set_status', status_code: 201 } },
      { send: { type: 'set_headers', headers: { 'content-type': 'text/plain' } } },
      { write: 'hello' },
      'close',
    ])
  })

  it('opens res.stream only when it is used, so a buffered response leaves the stream untouched', async () => {
    const buffered = fakeResponse()
    const result = await http(async () => ({ status_code: 200, body: { ok: true } }))({
      ...request,
      response: buffered.response,
    })
    expect(result).toEqual({ status_code: 200, body: { ok: true } })
    expect(buffered.sent).toEqual([])
    expect(buffered.toWritableCalls()).toBe(0)

    const streamed = fakeResponse()
    await http(async (_req, res) => {
      await new Promise<void>((resolve) => res.stream.end('piped', resolve))
    })({ ...request, response: streamed.response })
    expect(streamed.toWritableCalls()).toBe(1)
    expect(streamed.sent).toEqual([{ write: 'piped' }, 'close'])
  })

  it('passes the request without its response stream', async () => {
    const { response } = fakeResponse()
    let seen: Record<string, unknown> | undefined
    await http(async (req) => {
      seen = req as unknown as Record<string, unknown>
    })({ ...request, response })
    expect(seen).toMatchObject({ method: 'POST', body: { a: 1 } })
    expect(seen && 'response' in seen).toBe(false)
  })
})
