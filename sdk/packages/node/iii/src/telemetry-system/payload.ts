/** Payload redaction + truncation for invocation event capture. */

export const REDACTED_PLACEHOLDER = '[REDACTED]'
const TRUNCATION_MARKER = '..."[TRUNCATED]"'

export function resolveMaxBytesFromEnv(): number | null {
  const raw = process.env.III_TRACE_PAYLOAD_MAX_BYTES
  if (raw === undefined) return null
  const trimmed = raw.trim()
  if (trimmed === '' || trimmed.toLowerCase() === 'unlimited') return null
  const parsed = Number.parseInt(trimmed, 10)
  if (!Number.isFinite(parsed) || parsed <= 0) return null
  return parsed
}

const SENSITIVE_FRAGMENTS = [
  'api_key',
  'apikey',
  'api-key',
  'password',
  'secret',
  'credential',
  'authorization',
  'auth_token',
  'access_token',
  'refresh_token',
  'bearer',
  'private_key',
  'client_secret',
]

function isSensitiveKey(key: string): boolean {
  const lower = key.toLowerCase()
  if (SENSITIVE_FRAGMENTS.some((fragment) => lower.includes(fragment))) return true
  // `token` alone is too common a substring; require whole-key or suffix match.
  return lower === 'token' || lower.endsWith('_token') || lower.endsWith('-token')
}

/** Recursively redact values of sensitive keys. Returns a new value. */
export function redact(value: unknown): unknown {
  if (value === null || value === undefined) return value
  if (Array.isArray(value)) return value.map(redact)
  if (typeof value === 'object') {
    const out: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      out[k] = isSensitiveKey(k) ? REDACTED_PLACEHOLDER : redact(v)
    }
    return out
  }
  return value
}

/** Redact then serialize to JSON, optionally capped at `maxBytes`. */
export function redactAndTruncate(
  value: unknown,
  maxBytes: number | null = null,
): { json: string; truncated: boolean } {
  const redacted = redact(value)
  let serialized: string
  try {
    serialized = JSON.stringify(redacted) ?? 'null'
  } catch {
    serialized = 'null'
  }

  if (maxBytes === null || maxBytes === undefined || maxBytes <= 0) {
    return { json: serialized, truncated: false }
  }

  const byteLen = Buffer.byteLength(serialized, 'utf8')
  if (byteLen <= maxBytes) {
    return { json: serialized, truncated: false }
  }

  const cap = Math.max(0, maxBytes - Buffer.byteLength(TRUNCATION_MARKER, 'utf8'))
  const buf = Buffer.from(serialized, 'utf8')
  let cut = Math.min(cap, buf.length)
  // Walk back to a UTF-8 boundary so we don't emit half-codepoints.
  while (cut > 0 && (buf[cut] & 0xc0) === 0x80) {
    cut -= 1
  }
  const truncated = buf.subarray(0, cut).toString('utf8') + TRUNCATION_MARKER
  return { json: truncated, truncated: true }
}
