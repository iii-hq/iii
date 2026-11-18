import { Readable, Writable } from 'node:stream'
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest'
import { WebSocket } from 'ws'
import { InputStream, type OutputStream, StreamAbortedError } from '../src/index'
import { closeWorkers, sleep, worker } from './observe-e2e-utils'
import { engineWsUrl } from './utils'

/**
 * Call-native streams between this SDK and the browser SDK (iii-browser-sdk),
 * through a real engine (`III_URL`). The browser SDK is loaded from its source
 * in the workspace, by a non-literal path, so this package's `tsc` (no DOM
 * lib) leaves it out. Its sockets are the global `WebSocket`, which Node 20
 * does not have: `ws`'s stands in, as in the browser SDK's own integration
 * tests.
 */

type BrowserInput = { text(): Promise<string> }
type BrowserOutput = { write(chunk: string): Promise<void> }
type BrowserWorker = {
  registerFunction(id: string, handler: (input: never) => Promise<unknown>): unknown
  call<O = unknown>(request: { function_id: string; payload: unknown }): Promise<O>
  shutdown(): Promise<void>
}
type BrowserSdk = {
  InputStream: abstract new (...args: never[]) => BrowserInput
  OutputStream: abstract new (...args: never[]) => BrowserOutput
  StreamAbortedError: abstract new (...args: never[]) => Error
  registerWorker(url: string, options: Record<string, unknown>): BrowserWorker
}

const browserSdkPath = new URL('../../iii-browser/src/index.ts', import.meta.url).href
const RUN = Math.random().toString(16).slice(2, 10)
const fid = (name: string) => `browser-interop::${name}-${RUN}`
const decode = (b: Uint8Array) => new TextDecoder().decode(b)

let browser: BrowserSdk
const browserWorkers: BrowserWorker[] = []
const hadWebSocket = 'WebSocket' in globalThis
const ownWebSocket = (globalThis as { WebSocket?: unknown }).WebSocket

beforeAll(async () => {
  ;(globalThis as { WebSocket?: unknown }).WebSocket = WebSocket
  browser = (await import(/* @vite-ignore */ browserSdkPath)) as BrowserSdk
})

afterEach(async () => {
  for (const sdk of browserWorkers.splice(0)) await sdk.shutdown().catch(() => undefined)
  await closeWorkers()
})

afterAll(() => {
  if (hadWebSocket) (globalThis as { WebSocket?: unknown }).WebSocket = ownWebSocket
  else delete (globalThis as { WebSocket?: unknown }).WebSocket
})

/** A browser SDK worker of its own, closed after each test. */
function browserWorker(role: string): BrowserWorker {
  const sdk = browser.registerWorker(engineWsUrl, {
    workerName: `browser-${role}-${RUN}`,
    reconnectionConfig: { maxRetries: 20, initialDelayMs: 100, maxDelayMs: 500, jitterFactor: 0 },
  })
  browserWorkers.push(sdk)
  return sdk
}

/** `p`, or a rejection after `ms`: a call that never ends fails the test instead of hanging it. */
function within<T>(p: Promise<T>, ms = 5000): Promise<T> {
  return Promise.race([
    p,
    sleep(ms).then(() => {
      throw new Error(`no answer within ${ms} ms`)
    }),
  ]) as Promise<T>
}

/** A WritableStream that records what it gets, and how it ended. */
function webCollector(): { sink: WritableStream<Uint8Array>; text: () => string; state: () => string } {
  const chunks: Uint8Array[] = []
  let state = 'open'
  const sink = new WritableStream<Uint8Array>({
    write(chunk) {
      chunks.push(chunk)
    },
    close() {
      state = 'closed'
    },
    abort() {
      state = 'aborted'
    },
  })
  return { sink, text: () => chunks.map(decode).join(''), state: () => state }
}

function readableOf(chunks: string[]): ReadableStream<Uint8Array> {
  return new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(new TextEncoder().encode(chunk))
      controller.close()
    },
  })
}

describe('call-native streams between the Node SDK and the browser SDK', () => {
  it('a browser caller sends a ReadableStream and a WritableStream to a Node function, and gets a stream back', async () => {
    const host = worker('interop-host')
    host.registerFunction(fid('node-upper'), async ({ body, out }: { body: InputStream; out: OutputStream }) => {
      const text = await body.text()
      await out.write(text.toUpperCase())
      async function* echo() {
        yield `node:${text}`
      }
      return { len: text.length, echo: echo() }
    })
    const caller = browserWorker('interop-browser-caller')
    await sleep(300)
    const { sink, text, state } = webCollector()
    const result = await within(
      caller.call<{ len: number; echo: BrowserInput }>({
        function_id: fid('node-upper'),
        payload: { body: readableOf(['from ', 'browser']), out: sink },
      }),
    )
    expect(result.len).toBe(12)
    expect(state()).toBe('closed')
    expect(text()).toBe('FROM BROWSER')
    expect(result.echo).toBeInstanceOf(browser.InputStream)
    expect(await result.echo.text()).toBe('node:from browser')
  })

  it('a Node caller sends a Readable and a Writable to a browser function, and gets a stream back', async () => {
    const host = browserWorker('interop-browser-host')
    host.registerFunction(fid('browser-upper'), (async ({ body, out }: { body: BrowserInput; out: BrowserOutput }) => {
      expect(body).toBeInstanceOf(browser.InputStream)
      expect(out).toBeInstanceOf(browser.OutputStream)
      const text = await body.text()
      await out.write(text.toUpperCase())
      return { len: text.length, echo: readableOf([`browser:${text}`]) }
    }) as (input: never) => Promise<unknown>)
    const caller = worker('interop-node-caller')
    await sleep(300)
    const chunks: Buffer[] = []
    let finished = false
    const sink = new Writable({
      write(chunk, _encoding, callback) {
        chunks.push(chunk)
        callback()
      },
      final(callback) {
        finished = true
        callback()
      },
    })
    const result = await within(
      caller.call<unknown, { len: number; echo: InputStream }>({
        function_id: fid('browser-upper'),
        payload: { body: Readable.from([Buffer.from('from '), Buffer.from('node')]), out: sink },
      }),
    )
    expect(result.len).toBe(9)
    expect(finished).toBe(true)
    expect(Buffer.concat(chunks).toString()).toBe('FROM NODE')
    expect(result.echo).toBeInstanceOf(InputStream)
    expect(await result.echo.text()).toBe('browser:from node')
  })

  it('an abort from the browser (close code 4011) reaches a Node reader as STREAM_ABORTED, and the other way round', async () => {
    const nodeHost = worker('interop-abort-node')
    nodeHost.registerFunction(fid('node-read'), async ({ body }: { body: InputStream }) => {
      try {
        for await (const _ of body) {
          // reading
        }
      } catch (error) {
        return { code: (error as StreamAbortedError).code, reason: (error as StreamAbortedError).reason }
      }
      return { code: 'none' }
    })
    nodeHost.registerFunction(fid('node-abort-write'), async ({ out }: { out: OutputStream }) => {
      await out.write('x')
      await out.abort('node gave up')
      return 'returned'
    })
    const caller = browserWorker('interop-abort-browser')
    await sleep(300)

    async function* failing() {
      yield 'first'
      await sleep(30)
      throw new Error('browser source failed')
    }
    await expect(within(caller.call({ function_id: fid('node-read'), payload: { body: failing() } }))).resolves.toEqual(
      { code: 'STREAM_ABORTED', reason: 'channel writer aborted: closed with code 4011' },
    )

    const { sink, state } = webCollector()
    const error = await within(caller.call({ function_id: fid('node-abort-write'), payload: { out: sink } })).catch(
      (e: unknown) => e,
    )
    expect(error).toBeInstanceOf(browser.StreamAbortedError)
    expect(error).not.toBeInstanceOf(StreamAbortedError)
    expect(error).toMatchObject({ code: 'STREAM_ABORTED', reason: 'channel writer aborted: closed with code 1011' })
    for (let i = 0; i < 100 && state() !== 'aborted'; i++) await sleep(10)
    expect(state()).toBe('aborted')
  })
})
