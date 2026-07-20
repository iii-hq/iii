import type { StreamChannelRef } from './iii-types'

/**
 * Safely stringify a value, handling circular references, BigInt, and other edge cases.
 * Returns "[unserializable]" if serialization fails for any reason.
 */
export function safeStringify(value: unknown): string {
  const seen = new WeakSet<object>()

  try {
    return JSON.stringify(value, (_key, val) => {
      if (typeof val === 'bigint') {
        return val.toString()
      }

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

/**
 * Recursively extract all {@link StreamChannelRef} values from a JSON-like
 * input, returning each match paired with its dotted/bracketed path. Mirrors
 * the Rust SDK's `extract_channel_refs`.
 *
 * @param data - Arbitrary JSON-like value.
 * @returns Array of `[path, ref]` tuples. Empty when no refs are found.
 */
export const extractChannelRefs = (data: unknown): Array<[string, StreamChannelRef]> => {
  const refs: Array<[string, StreamChannelRef]> = []
  extractRefsRecursive(data, '', refs)
  return refs
}

const extractRefsRecursive = (
  data: unknown,
  prefix: string,
  refs: Array<[string, StreamChannelRef]>,
): void => {
  if (isChannelRef(data)) {
    refs.push([prefix, data])
    return
  }
  if (Array.isArray(data)) {
    for (let i = 0; i < data.length; i++) {
      const path = prefix === '' ? `[${i}]` : `${prefix}[${i}]`
      extractRefsRecursive(data[i], path, refs)
    }
    return
  }
  if (typeof data !== 'object' || data === null) return

  for (const [key, value] of Object.entries(data as Record<string, unknown>)) {
    const path = prefix === '' ? key : `${prefix}.${key}`
    extractRefsRecursive(value, path, refs)
  }
}

/**
 * Generate an RFC 4122 v4 UUID.
 *
 * `crypto.randomUUID` is only defined in secure contexts (https / localhost),
 * so pages served over plain http from a LAN address would throw on every
 * invocation. Fall back to building the UUID from `crypto.getRandomValues`,
 * which is available in insecure contexts and uses the same CSPRNG. The
 * engine parses invocation ids as UUIDs, so the fallback must be a real one.
 */
export function randomUUID(): string {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  bytes[6] = (bytes[6] & 0x0f) | 0x40
  bytes[8] = (bytes[8] & 0x3f) | 0x80
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}
