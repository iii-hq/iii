/**
 * Type-level checks of the call-native stream API. Not executed at runtime
 * (vitest only picks up `*.test.ts`), but `tsc --noEmit` compiles it.
 */
import type { Readable, Writable } from 'node:stream'
import { type HttpRequest, http } from '@iii-dev/helpers/http'
import type { Pipe, PipeOptions, StreamInfo, StreamRequest, StreamResponse } from '../src/index'
import { InputStream, OutputStream, registerWorker, StreamAbortedError } from '../src/index'
import type { StreamChannelRef } from '../src/protocol'
import { ChannelDirection, ChannelItem, extractChannelRefs, isChannelRef } from '../src/protocol'
import type { InternalHttpRequest } from '../src/types'

declare const readable: Readable
declare const writable: Writable

// biome-ignore lint/correctness/noUnusedVariables: compile-only assertions
async function streamTypingAssertions() {
  const worker = registerWorker('ws://localhost:49134')

  // A source and a sink go in a payload as they are.
  await worker.call({ function_id: 'files::process', payload: { body: readable, out: writable } })

  // pipe() gives both ends.
  const options: PipeOptions = { bufferSize: 16 }
  const pipe: Pipe = await worker.pipe(options)
  const input: InputStream = pipe.input
  const output: OutputStream = pipe.output
  await output.write('text')
  await output.write(new Uint8Array([1]))
  await output.send({ type: 'progress' })
  output.keepOpen().toWritable()
  await output.abort(new Error('boom'))

  for await (const chunk of input) {
    const b: Buffer = chunk
    void b
  }
  const info: StreamInfo = input.toJSON()
  void info
  for await (const message of InputStream.from('x').messages<{ type: string }>()) void message.type
  for await (const frame of InputStream.from('x').frames()) {
    if (frame.type === 'binary') void (frame.value satisfies Buffer)
    else void (frame.value satisfies string)
  }
  void OutputStream.to(writable)

  // An InputStream is what the HTTP helpers read the raw body with.
  const body: HttpRequest['request_body'] = InputStream.from('raw')
  void body
  const internal: InternalHttpRequest['request_body'] = body as InputStream
  void internal
  const streamBody: StreamRequest['request_body'] = InputStream.from('raw')
  void streamBody

  // The helpers' http() response is the SDK's StreamResponse.
  worker.registerFunction(
    'http::upload',
    http(async (req, res: StreamResponse) => {
      await res.status(201)
      await res.headers({ 'content-type': 'text/plain' })
      await res.write(await req.request_body.bytes())
      await res.close()
    }),
  )

  // The abort error carries its reason.
  const error = new StreamAbortedError('reason')
  const reason: string = error.reason
  void reason

  // The wire helpers live in iii-sdk/protocol.
  const ref: StreamChannelRef = { channel_id: 'c', access_key: 'k', direction: ChannelDirection.Read }
  void isChannelRef(ref)
  void extractChannelRefs({ ref })
  void ChannelItem.Text('hi')
}
