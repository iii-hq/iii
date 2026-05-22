import { context as otelContext, propagation, SpanKind, SpanStatusCode, trace, type Tracer } from '@opentelemetry/api'

const SAFE_REQUEST_HEADERS = ['content-type', 'accept'] as const
const SAFE_RESPONSE_HEADERS = ['content-type'] as const

export interface TracedFetchInit extends RequestInit {
  tracer?: Tracer
}

/**
 * Execute a fetch request inside an OTel CLIENT span.
 *
 * Mirrors the Rust execute_traced_request shape: injects W3C traceparent into
 * outgoing headers, records HTTP semantic-convention attributes, and sets
 * ERROR span status for HTTP responses with status >= 400 or network errors.
 */
export async function executeTracedRequest(
  input: RequestInfo | URL,
  init?: TracedFetchInit,
): Promise<Response> {
  const tracer = init?.tracer ?? trace.getTracer('iii-node-sdk')
  const rawUrl = typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url
  let url: URL | null
  try {
    url = new URL(rawUrl)
  } catch {
    url = null
  }
  const method = (init?.method ?? (typeof input === 'object' && 'method' in input ? input.method : 'GET') ?? 'GET').toUpperCase()
  const name = url?.pathname ? `${method} ${url.pathname}` : method

  return tracer.startActiveSpan(
    name,
    {
      kind: SpanKind.CLIENT,
      attributes: {
        'http.request.method': method,
        'url.full': url?.toString() ?? rawUrl,
        'network.protocol.name': 'http',
        ...(url
          ? {
              'server.address': url.hostname,
              'url.scheme': url.protocol.replace(':', ''),
              'url.path': url.pathname,
            }
          : {}),
        ...(url?.port ? { 'server.port': Number(url.port) } : {}),
        ...(url?.search ? { 'url.query': url.search.slice(1) } : {}),
      },
    },
    async (span) => {
      try {
        const headers = new Headers(init?.headers)
        const carrier: Record<string, string> = {}
        propagation.inject(otelContext.active(), carrier)
        for (const [k, v] of Object.entries(carrier)) headers.set(k, v)

        for (const h of SAFE_REQUEST_HEADERS) {
          const v = headers.get(h)
          if (v) span.setAttribute(`http.request.header.${h}`, v)
        }

        const response = await fetch(input, { ...init, headers })
        span.setAttribute('http.response.status_code', response.status)
        const cl = response.headers.get('content-length')
        if (cl) span.setAttribute('http.response.body.size', Number(cl))
        for (const h of SAFE_RESPONSE_HEADERS) {
          const v = response.headers.get(h)
          if (v) span.setAttribute(`http.response.header.${h}`, v)
        }

        if (response.status >= 400) {
          span.setStatus({ code: SpanStatusCode.ERROR, message: String(response.status) })
          span.setAttribute('error.type', String(response.status))
        } else {
          span.setStatus({ code: SpanStatusCode.OK })
        }
        return response
      } catch (err) {
        const error = err as Error
        span.recordException(error)
        span.setStatus({ code: SpanStatusCode.ERROR, message: error.message })
        span.setAttribute('error.type', error.name)
        throw err
      } finally {
        span.end()
      }
    },
  )
}
