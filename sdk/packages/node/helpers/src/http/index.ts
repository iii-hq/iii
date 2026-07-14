/**
 * HTTP method accepted by {@link HttpInvocationConfig}. Distinct from the core
 * `builtin_triggers` HTTP method enum, which also covers HEAD/OPTIONS.
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
 * Configuration for registering an HTTP-invoked function (Lambda, Cloudflare
 * Workers, etc.) instead of a local handler.
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
  path_params: Record<string, string>
  query_params: Record<string, string | string[]>
  body: TBody
  headers: Record<string, string | string[]>
  method: string
  request_body: HttpStreamReader
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
export type HttpResponse<
  TStatus extends number = number,
  TBody = string | Buffer | Record<string, unknown>,
> = {
  /** HTTP status code. */
  status_code: TStatus
  /** Response headers. */
  headers?: Record<string, string>
  /** Response body. */
  body?: TBody
}

/**
 * Structural shape of the reader end of a stream channel. Declared locally so
 * the helpers package does not runtime-depend on the core SDK.
 */
type HttpStreamReader = {
  stream: NodeJS.ReadableStream
  readAll: () => Promise<Buffer>
  onMessage: (callback: (msg: string) => void) => void
  close: () => void
}

/** Structural shape of the writer end of a stream channel. */
type HttpStreamWriter = {
  sendMessage: (message: string) => unknown
  stream: NodeJS.WritableStream
  close: () => unknown
}

/** Structural shape of a streaming request passed to the {@link http} handler. */
type HttpStreamingRequest = {
  path_params: Record<string, string>
  query_params: Record<string, string | string[]>
  body: unknown
  headers: Record<string, string | string[]>
  method: string
  request_body: HttpStreamReader
}

/** Structural shape of the streaming response passed to the {@link http} handler. */
type HttpStreamingResponse = {
  status: (statusCode: number) => void
  headers: (headers: Record<string, string>) => void
  stream: NodeJS.WritableStream
  close: () => void
}

/** Structural shape of the internal request delivered by the runtime. */
type HttpInternalRequest = HttpStreamingRequest & { response: HttpStreamWriter }

/**
 * Helper that wraps an HTTP-style handler (with separate `req`/`res` arguments)
 * into the function handler format expected by the SDK.
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
 *     res.status(200)
 *     res.headers({ 'content-type': 'application/json' })
 *     res.stream.end(JSON.stringify({ hello: 'world' }))
 *     res.close()
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

    const httpResponse: HttpStreamingResponse = {
      status: (status_code: number) =>
        response.sendMessage(JSON.stringify({ type: 'set_status', status_code })),
      headers: (headers: Record<string, string>) =>
        response.sendMessage(JSON.stringify({ type: 'set_headers', headers })),
      stream: response.stream,
      close: () => response.close(),
    }

    return callback(request, httpResponse)
  }
}
