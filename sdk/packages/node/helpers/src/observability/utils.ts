/**
 * Safely stringify a value, handling circular references, BigInt, and other edge cases.
 * Returns "[unserializable]" if serialization fails for any reason.
 *
 * @param value - The value to serialize to JSON.
 */
export function safeStringify(value: unknown): string {
  const seen = new WeakSet<object>()

  try {
    const result = JSON.stringify(value, (_key, val) => {
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
    return result ?? '[unserializable]'
  } catch {
    return '[unserializable]'
  }
}
