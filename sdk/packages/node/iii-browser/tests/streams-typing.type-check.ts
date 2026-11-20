/**
 * Type-level checks of the call-native stream API. Not executed at runtime
 * (vitest only picks up `*.test.ts`), but `tsc --noEmit` compiles it.
 */
import type { InputStreamSource, OutputStreamSink, Pipe, PipeOptions, StreamFrame, StreamInfo } from '../src/index'
import { InputStream, OutputStream, registerWorker, StreamAbortedError } from '../src/index'
import type { StreamChannelRef } from '../src/protocol'
import { ChannelDirection, ChannelItem, extractChannelRefs, isChannelRef } from '../src/protocol'

declare const readable: ReadableStream<Uint8Array>
declare const writable: WritableStream<Uint8Array>
declare const file: File

// biome-ignore lint/correctness/noUnusedVariables: compile-only assertions
async function streamTypingAssertions() {
  const worker = registerWorker('ws://localhost:49134')

  // A source and a sink go in a payload as they are.
  await worker.call({ function_id: 'files::process', payload: { body: readable, out: writable } })
  await worker.call({ function_id: 'files::upload', payload: { body: file } })

  // pipe() gives both ends.
  const options: PipeOptions = { bufferSize: 16 }
  const pipe: Pipe = await worker.pipe(options)
  const input: InputStream = pipe.input
  const output: OutputStream = pipe.output
  await output.write('text')
  await output.write(new Uint8Array([1]))
  await output.send({ type: 'progress' })
  const web: WritableStream<Uint8Array> = output.keepOpen().toWebStream()
  void web
  await output.abort(new Error('boom'))

  for await (const chunk of input) {
    const b: Uint8Array = chunk
    void b
  }
  const info: StreamInfo = input.toJSON()
  void info
  const body: ReadableStream<Uint8Array> = InputStream.from('x').toWebStream()
  void body
  for await (const message of InputStream.from('x').messages<{ type: string }>()) void message.type
  for await (const frame of InputStream.from('x').frames()) {
    const f: StreamFrame = frame
    if (f.type === 'binary') void (f.value satisfies Uint8Array)
    else void (f.value satisfies string)
  }
  const sources: InputStreamSource[] = [readable, file, new Blob([]), InputStream.from('x')]
  void sources
  const sinks: OutputStreamSink[] = [writable, OutputStream.to(writable).toWebStream()]
  void sinks

  // The abort error carries its reason.
  const error = new StreamAbortedError('reason')
  const reason: string = error.reason
  void reason

  // The wire helpers live in iii-browser-sdk/protocol.
  const ref: StreamChannelRef = { channel_id: 'c', access_key: 'k', direction: ChannelDirection.Read }
  void isChannelRef(ref)
  void extractChannelRefs({ ref })
  void ChannelItem.Text('hi')
}
