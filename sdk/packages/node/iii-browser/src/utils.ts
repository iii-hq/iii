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
    if (isChannelRef(value)) {
      refs.push([path, value])
    } else if (Array.isArray(value)) {
      for (let i = 0; i < value.length; i++) {
        extractRefsRecursive(value[i], `${path}[${i}]`, refs)
      }
    } else if (typeof value === 'object' && value !== null) {
      extractRefsRecursive(value, path, refs)
    }
  }
}
