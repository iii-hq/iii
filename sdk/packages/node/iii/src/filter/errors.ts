/** Structural limits (filters.md §4, fixtures README "Limits"). */
export const MAX_DEPTH = 8
export const MAX_NODES = 64
export const MAX_SET_ITEMS = 256
export const MAX_PATH_SEGMENTS = 16
export const MAX_ENCODED_BYTES = 8192
/** Arrays / objects nested in one `$eq` / `$ne` operand or one `$in` / `$nin` item. */
export const MAX_LITERAL_DEPTH = 32
/**
 * Arrays / objects nested in the whole filter (a shared rule, with vectors).
 * The engine's JSON decoder reads 128 levels per message and a filter sits a
 * few levels into its `observe` message (about 124 left), so filters nesting
 * deeper than this cannot be delivered reliably.
 */
export const MAX_JSON_DEPTH = 120
/** Numeric literals in a filter must satisfy `|n| <= 2^53`. */
export const MAX_SAFE_LITERAL = 2 ** 53

export type FilterErrorCode = 'INVALID_FILTER' | 'INVALID_FILTER_PATH'

/** A filter the language rejects. `code` is the protocol error code (protocol.md §4). */
export class FilterError extends Error {
  readonly code: FilterErrorCode

  constructor(code: FilterErrorCode, message: string) {
    super(message)
    this.name = 'FilterError'
    this.code = code
  }

  static invalid(message: string): FilterError {
    return new FilterError('INVALID_FILTER', message)
  }
}
