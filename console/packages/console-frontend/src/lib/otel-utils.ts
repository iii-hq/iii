/**
 * OTEL Utility Functions
 * Provides timestamp conversion and attribute normalization utilities
 * for working with OpenTelemetry data formats.
 */

/**
 * Convert nanoseconds to milliseconds
 * @param nanos - Timestamp in nanoseconds
 * @returns Timestamp in milliseconds (floored)
 */
export function nanoToMs(nanos: number): number {
  // biome-ignore lint/suspicious/noGlobalIsFinite: isFinite is safe for number type
  if (!isFinite(nanos)) {
    return 0
  }
  return Math.floor(nanos / 1_000_000)
}

/**
 * Normalize attributes from tuple array or object format to object format
 * Supports both OpenTelemetry tuple array format and object format
 * @param attrs - Attributes as tuple array or object, or null/undefined
 * @returns Normalized attributes as object
 */
export function normalizeAttributes(
  attrs: Array<[string, string]> | Record<string, unknown> | null | undefined,
): Record<string, unknown> {
  // Handle null or undefined
  if (attrs == null) {
    return {}
  }

  // Handle array format (tuple array)
  if (Array.isArray(attrs)) {
    if (attrs.length === 0) {
      return {}
    }

    const result: Record<string, unknown> = {}
    for (const item of attrs) {
      if (Array.isArray(item) && item.length >= 2) {
        const [key, value] = item
        if (typeof key === 'string') {
          result[key] = value
        }
      }
    }
    return result
  }

  // Handle object format
  if (typeof attrs === 'object') {
    return attrs as Record<string, unknown>
  }

  // Fallback for unexpected types
  return {}
}
