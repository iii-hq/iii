import * as fs from 'node:fs'
import * as path from 'node:path'
import type { StreamChannelRef } from './iii-types'
import type { ApiResponse, HttpRequest, HttpResponse, InternalHttpRequest } from './types'

/**
 * Returns a project identifier for telemetry, derived from the current working
 * directory. Reads `package.json` `name` if present at `cwd`; otherwise falls
 * back to the basename of `cwd`. Returns `undefined` only when both signals
 * are unavailable (e.g. cwd is the filesystem root).
 *
 * No directory walking — only inspects `cwd` itself, so the SDK never reads
 * files outside the user's explicit working directory.
 */
export function detectProjectName(cwd: string = process.cwd()): string | undefined {
  try {
    const manifest = path.join(cwd, 'package.json')
    if (fs.existsSync(manifest)) {
      const parsed = JSON.parse(fs.readFileSync(manifest, 'utf8')) as { name?: unknown }
      if (typeof parsed.name === 'string') {
        const trimmed = parsed.name.trim()
        if (trimmed) return trimmed
      }
    }
  } catch {
    // fall through to directory-name fallback
  }

  const base = path.basename(cwd).trim()
  return base || undefined
}

/**
 * Safely stringify a value, handling circular references, BigInt, and other edge cases.
 * Returns "[unserializable]" if serialization fails for any reason.
 */
export function safeStringify(value: unknown): string {
  const seen = new WeakSet<object>()

  try {
    return JSON.stringify(value, (_key, val) => {
      // Handle BigInt
      if (typeof val === 'bigint') {
        return val.toString()
      }

      // Handle circular references
      if (val !== null && typeof val === 'object') {
        if (seen.has(val)) {
          return '[Circular]'
        }
        seen.add(val)
      }

      return val
    })
  } catch {
    return '[unserializable]'
  }
}

/**
 * Helper that wraps an HTTP-style handler (with separate `req`/`res` arguments)
 * into the function handler format expected by the SDK.
 *
 * @param callback - Async handler receiving an {@link HttpRequest} and {@link HttpResponse}.
 * @returns A function handler compatible with {@link ISdk.registerFunction}.
 *
 * @example
 * ```typescript
 * import { http } from 'iii-sdk'
 *
 * iii.registerFunction(
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
  callback: (req: HttpRequest, res: HttpResponse) => Promise<void | ApiResponse>,
) => {
  return async (req: InternalHttpRequest) => {
    const { response, ...request } = req

    const httpResponse: HttpResponse = {
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

/**
 * Type guard that checks if a value is a {@link StreamChannelRef}.
 *
 * @param value - Value to check.
 * @returns `true` if the value is a valid `StreamChannelRef`.
 */
export const isChannelRef = (value: unknown): value is StreamChannelRef => {
  if (typeof value !== 'object' || value === null) return false
  const maybe = value as Partial<StreamChannelRef>
  return (
    typeof maybe.channel_id === 'string' &&
    typeof maybe.access_key === 'string' &&
    (maybe.direction === 'read' || maybe.direction === 'write')
  )
}
