/**
 * Extract a human-readable message from the Rust `ErrorBody` debug format:
 * 'ErrorBody { code: "invocation_failed", message: "Simulated failure", stacktrace: ... }'
 *
 * The message may contain escaped quotes — worker-op errors embed a JSON
 * envelope like {"type":"WorkerOpError","code":"W105","message":"..."} — so
 * the regex matches escaped sequences instead of stopping at the first inner
 * quote, and JSON envelopes are unwrapped to `code: message`.
 */
/** DLQ error strings are worker/handler-supplied and render one per row, so
 * every path is length-capped to keep an oversized message out of the DOM. */
const MAX_ERROR_LEN = 100

function truncate(s: string): string {
  return s.length > MAX_ERROR_LEN ? `${s.slice(0, MAX_ERROR_LEN)}...` : s
}

export function extractErrorMessage(error: string): string {
  const msgMatch = error.match(/message:\s*"((?:[^"\\]|\\.)*)"/)
  if (msgMatch) {
    return truncate(humanizeEnvelope(unescapeJsonString(msgMatch[1])))
  }
  // The error may already be the bare JSON envelope (no Debug wrapper).
  const direct = humanizeEnvelope(error)
  if (direct !== error) return truncate(direct)
  // Fallback: a plain string, returned as-is (capped).
  return truncate(error)
}

/**
 * Decode the escape sequences captured from the Rust Debug `message: "..."`
 * field. The capture is the raw inner content (escapes intact), so parsing it
 * as a JSON string preserves `\n`, `\t`, `\uXXXX` instead of stripping the
 * backslash from every escape. Malformed input falls back to a minimal
 * quote-unescape so we never throw on worker-supplied text.
 */
function unescapeJsonString(inner: string): string {
  try {
    return JSON.parse(`"${inner}"`) as string
  } catch {
    return inner.replace(/\\"/g, '"')
  }
}

/** If `raw` is a worker-op JSON envelope, surface `code: message`; otherwise return it unchanged. */
function humanizeEnvelope(raw: string): string {
  if (!raw.startsWith('{')) return raw
  try {
    const parsed = JSON.parse(raw) as { code?: unknown; message?: unknown }
    if (typeof parsed.message === 'string') {
      return typeof parsed.code === 'string' ? `${parsed.code}: ${parsed.message}` : parsed.message
    }
  } catch {
    // not JSON — fall through
  }
  return raw
}
