/**
 * HTTP method accepted by {@link HttpInvocationConfig}. Distinct from the
 * methods an `http::on-request` route takes, which also cover HEAD/OPTIONS.
 */
export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'

/**
 * Authentication configuration for HTTP-invoked functions.
 *
 * - `hmac`: HMAC signature verification using a shared secret.
 * - `bearer`: Bearer token authentication.
 * - `api_key`: API key sent via a custom header.
 */
export type HttpAuthConfig =
  | { type: 'hmac'; secret_key: string }
  | { type: 'bearer'; token_key: string }
  | { type: 'api_key'; header: string; value_key: string }

/**
 * Configuration for an HTTP-invoked function (Lambda, Cloudflare Workers, etc.).
 *
 * @deprecated HttpInvocationConfig is deprecated (iii-http-functions) and will be removed in an upcoming release. Behavior is unchanged for now. Migration guide: https://iii.dev/docs/upgrading/migrate-from-http-functions
 */
export type HttpInvocationConfig = {
  /** URL to invoke. */
  url: string
  /** HTTP method. Defaults to `POST`. */
  method?: HttpMethod
  /** Timeout in milliseconds. */
  timeout_ms?: number
  /** Custom headers to send with the request. */
  headers?: Record<string, string>
  /** Authentication configuration. */
  auth?: HttpAuthConfig
}

/**
 * Incoming buffered HTTP request received by a function handler.
 *
 * @typeParam TBody - Type of the parsed request body.
 */
export type HttpRequest<TBody = unknown> = {
  /** Path parameters extracted from the matched route. */
  path_params: Record<string, string>
  /** Query-string parameters from the request URL. */
  query_params: Record<string, string | string[]>
  /** Parsed request body. */
  body: TBody
  /** Request headers. */
  headers: Record<string, string | string[]>
  /** HTTP method of the request (e.g. `GET`, `POST`). */
  method: string
  /** The raw request body, as an `InputStream` of `iii-sdk`: read it, or pass it on unread. */
  request_body: HttpInputStream
}

/**
 * Structured buffered HTTP response returned from function handlers.
 *
 * @typeParam TStatus - HTTP status code literal type.
 * @typeParam TBody - Type of the response body.
 *
 * @example
 * ```typescript
 * const response: HttpResponse = {
 *   status_code: 200,
 *   headers: { 'content-type': 'application/json' },
 *   body: { message: 'ok' },
 * }
 * ```
 */
export type HttpResponse<TStatus extends number = number, TBody = string | Buffer | Record<string, unknown>> = {
  /** HTTP status code. */
  status_code: TStatus
  /** Response headers. */
  headers?: Record<string, string>
  /** Response body. */
  body?: TBody
}

/** One frame of {@link HttpInputStream.frames}: bytes, or a text message. */
type HttpStreamFrame = { type: 'binary'; value: Buffer } | { type: 'text'; value: string }

/**
 * Structural shape of the `InputStream` of `iii-sdk` (the read side of a
 * stream). Declared locally so the helpers package does not depend on the
 * core SDK.
 */
type HttpInputStream = AsyncIterable<Buffer> & {
  bytes: () => Promise<Buffer>
  text: () => Promise<string>
  lines: () => AsyncIterableIterator<string>
  messages: <T = unknown>() => AsyncIterableIterator<T>
  frames: () => AsyncIterableIterator<HttpStreamFrame>
  cancel: () => Promise<void>
  tee: () => [HttpInputStream, HttpInputStream]
  toReadable: () => NodeJS.ReadableStream
  toWebStream: () => ReadableStream<Uint8Array>
}

/** Structural shape of the `OutputStream` of `iii-sdk` (the write side of a stream). */
type HttpOutputStream = {
  write: (chunk: Uint8Array | string) => Promise<void>
  send: (message: unknown) => Promise<void>
  close: () => Promise<void>
  abort: (reason?: string | Error) => Promise<void>
  keepOpen: () => HttpOutputStream
  toWritable: () => NodeJS.WritableStream
  toWebStream: () => WritableStream<Uint8Array>
}

/** Structural shape of a streaming request passed to the {@link http} handler. */
type HttpStreamingRequest = {
  path_params: Record<string, string>
  query_params: Record<string, string | string[]>
  body: unknown
  headers: Record<string, string | string[]>
  method: string
  request_body: HttpInputStream
}

/** Structural shape of the streaming response passed to the {@link http} handler. */
type HttpStreamingResponse = {
  /** Sets the status code: a `set_status` message on the response stream. */
  status: (statusCode: number) => Promise<void>
  /** Sets response headers: a `set_headers` message on the response stream. */
  headers: (headers: Record<string, string>) => Promise<void>
  /** Writes a chunk of the body. */
  write: (chunk: Uint8Array | string) => Promise<void>
  /** A Writable for the body (pipe into it); ending it ends the response. */
  stream: NodeJS.WritableStream
  /** Ends the response. */
  close: () => Promise<void>
}

/** Structural shape of the internal request delivered by the runtime. */
type HttpInternalRequest = HttpStreamingRequest & { response: HttpOutputStream }

/** `p`, marked handled: a status or header that is not awaited does not crash the process when it fails. */
function quiet(p: Promise<void>): Promise<void> {
  p.catch(() => undefined)
  return p
}

/**
 * Helper that wraps an HTTP-style handler (with separate `req`/`res` arguments)
 * into the function handler format expected by the SDK. `res.status()` and
 * `res.headers()` send the `set_status` / `set_headers` messages the HTTP
 * worker reads; the body goes through `res.write()` or `res.stream`. The SDK
 * closes a response the handler used when it returns, and aborts it when it
 * throws; return an `HttpResponse` instead for a buffered response.
 *
 * @param callback - Async handler receiving a streaming request and response.
 * @returns A function handler compatible with `IIIClient.registerFunction`.
 *
 * @example
 * ```typescript
 * import { http } from '@iii-dev/helpers/http'
 *
 * worker.registerFunction(
 *   'my-api',
 *   http(async (req, res) => {
 *     await res.status(200)
 *     await res.headers({ 'content-type': 'application/json' })
 *     await res.write(JSON.stringify({ hello: 'world' }))
 *   }),
 * )
 * ```
 */
export const http = (
  // biome-ignore lint/suspicious/noConfusingVoidType: void is necessary here
  callback: (req: HttpStreamingRequest, res: HttpStreamingResponse) => Promise<void | HttpResponse>,
) => {
  return async (req: HttpInternalRequest) => {
    const { response, ...request } = req
    let stream: NodeJS.WritableStream | undefined

    const httpResponse: HttpStreamingResponse = {
      status: (status_code: number) => quiet(response.send({ type: 'set_status', status_code })),
      headers: (headers: Record<string, string>) => quiet(response.send({ type: 'set_headers', headers })),
      write: (chunk: Uint8Array | string) => quiet(response.write(chunk)),
      // Opened on first use: a response the handler never touches stays untouched.
      get stream() {
        stream ??= response.toWritable()
        return stream
      },
      close: () => quiet(response.close()),
    }

    return callback(request, httpResponse)
  }
}
