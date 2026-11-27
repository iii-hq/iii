/** Payload redaction + truncation for invocation event capture. */

export const REDACTED_PLACEHOLDER = '[REDACTED]'
const TRUNCATION_MARKER = '..."[TRUNCATED]"'

/**
 * Resolve the payload byte cap from the environment, returning undefined when
 * unset.
 */
export function resolveMaxBytesFromEnv(): number | null {
  const raw = process.env.III_TRACE_PAYLOAD_MAX_BYTES
  if (raw === undefined) return null
  const trimmed = raw.trim()
  if (trimmed === '' || trimmed.toLowerCase() === 'unlimited') return null
  // parseInt accepts partial parses like "8192mb" → 8192; Python int()
  // and Rust parse::<usize>() reject those. Match the strict semantics.
  if (!/^\d+$/.test(trimmed)) return null
  const parsed = Number(trimmed)
  if (parsed <= 0) return null
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

/**
 * Query parameters that carry a credential in a signed URL (S3 and R2
 * SigV2/SigV4, GCS, Azure SAS, CloudFront, the storage worker's local
 * provider), lowercased. The engine and every SDK carry the same list.
 */
const SENSITIVE_URL_PARAMS = new Set([
  'token',
  'sig',
  'signature',
  'policy',
  'key-pair-id',
  'awsaccesskeyid',
  'x-amz-signature',
  'x-amz-credential',
  'x-amz-security-token',
  'x-goog-signature',
  'x-goog-credential',
])

function isSensitiveUrlParam(name: string): boolean {
  return isSensitiveKey(name) || SENSITIVE_URL_PARAMS.has(name.toLowerCase())
}

const URL_SCHEME = /^[a-z][a-z0-9+.-]*:\/\//i

/**
 * Replaces the credentials in `text` with the placeholder when it is a URL:
 * one that starts with `scheme://` or `/`. The password of its userinfo goes,
 * or the whole userinfo when it has no `:` (a token used as the user name);
 * the authority runs to the first `/`, `?` or `#`, the userinfo to the last
 * `@` in it. So does the value of every query parameter whose name is
 * sensitive (the query runs from the first `?` to the next `#`). Everything
 * else is kept as it is; empty and already redacted values are skipped.
 */
function redactUrl(text: string): string {
  let authorityStart = -1
  if (!text.startsWith('/')) {
    const scheme = URL_SCHEME.exec(text)
    if (scheme === null) return text
    authorityStart = scheme[0].length
  }
  const secrets: Array<[number, number]> = []
  const secret = (start: number, end: number) => {
    const value = text.slice(start, end)
    if (value !== '' && value !== REDACTED_PLACEHOLDER) secrets.push([start, end])
  }
  if (authorityStart !== -1) {
    let authorityEnd = text.length
    for (let i = authorityStart; i < text.length; i++) {
      const char = text[i]
      if (char === '/' || char === '?' || char === '#') {
        authorityEnd = i
        break
      }
    }
    const at = text.lastIndexOf('@', authorityEnd - 1)
    if (at >= authorityStart) {
      const colon = text.indexOf(':', authorityStart)
      if (colon !== -1 && colon < at) secret(colon + 1, at)
      else secret(authorityStart, at)
    }
  }
  const question = text.indexOf('?')
  if (question !== -1) {
    const hash = text.indexOf('#', question + 1)
    const queryEnd = hash === -1 ? text.length : hash
    let paramStart = question + 1
    for (const param of text.slice(question + 1, queryEnd).split('&')) {
      const equals = param.indexOf('=')
      if (equals !== -1 && isSensitiveUrlParam(param.slice(0, equals))) {
        secret(paramStart + equals + 1, paramStart + param.length)
      }
      paramStart += param.length + 1
    }
  }
  if (secrets.length === 0) return text
  let out = ''
  let copied = 0
  for (const [start, end] of secrets) {
    out += text.slice(copied, start) + REDACTED_PLACEHOLDER
    copied = end
  }
  return out + text.slice(copied)
}

/**
 * Recursively redact values of sensitive keys, and the credentials in string
 * values that are URLs: the password of the userinfo (the whole userinfo when
 * it has no password) and the values of query parameters with a sensitive
 * name, such as the signature of a presigned URL.
 * Returns a new value.
 *
 * @param value - The value to recursively redact.
 */
export function redact(value: unknown): unknown {
  if (value === null || value === undefined) return value
  if (typeof value === 'string') return redactUrl(value)
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

/**
 * Redact then serialize to JSON, optionally capped at `maxBytes`.
 *
 * @param value - The value to redact and serialize.
 * @param maxBytes - Maximum serialized size in bytes; null leaves it uncapped.
 */
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

  const markerLen = Buffer.byteLength(TRUNCATION_MARKER, 'utf8')
  // When maxBytes is smaller than the marker itself, emit only a
  // truncated marker so the result never exceeds the cap.
  if (maxBytes <= markerLen) {
    return { json: TRUNCATION_MARKER.slice(0, maxBytes), truncated: true }
  }

  const cap = maxBytes - markerLen
  const buf = Buffer.from(serialized, 'utf8')
  let cut = Math.min(cap, buf.length)
  // Walk back to a UTF-8 boundary so we don't emit half-codepoints.
  while (cut > 0 && (buf[cut] & 0xc0) === 0x80) {
    cut -= 1
  }
  const truncated = buf.subarray(0, cut).toString('utf8') + TRUNCATION_MARKER
  return { json: truncated, truncated: true }
}
