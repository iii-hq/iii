/**
 * The observer filter language (`iii-filter` v1, filters.md): validation and
 * evaluation for the SDKs, with the same semantics as the engine's Rust
 * evaluator (`crates/iii-filter`).
 *
 * Pure TypeScript with no Node.js APIs: the browser SDK uses these files too.
 */
import { compile } from './compile'
import type { FilterDoc, Root } from './path'

export { FilterError, type FilterErrorCode } from './errors'
export type { FilterDoc, Root } from './path'

/** A filter object (filters.md §2): paths with explicit operators, `$and`, `$or`, `$not`. */
export type FilterSource = Record<string, unknown>

/** A validated filter, compiled once into a closure tree. */
export interface CompiledFilter {
  /** The filter as JSON (what travels on the wire). */
  readonly source: FilterSource
  /** `{}`: every document matches. */
  readonly matchAll: boolean
  /** The roots any clause references (`input`, `output`, `error`, `caller`, `metadata`). */
  readonly roots: ReadonlySet<Exclude<Root, '@'>>
  /**
   * Evaluates the filter on a document. Total: it never throws. Host values
   * are read as JSON would write them (`undefined` is absent, a `Date` is its
   * ISO string, a `BigInt` its decimal value); a document whose reading throws
   * (a throwing getter or `toJSON`, a Proxy trap, a revoked proxy) does not
   * match.
   */
  matches(doc: FilterDoc): boolean
}

export type CompileOptions = {
  /**
   * The filter was validated by the engine (the normal form it pushes to a
   * function's owner): skip the encoded size limit, which the engine already
   * applied and which JavaScript can measure slightly differently.
   */
  trusted?: boolean
}

/**
 * Validates and compiles a filter. Throws {@link FilterError}
 * (`INVALID_FILTER`) when it is not valid.
 *
 * @example
 * ```typescript
 * const f = compileFilter({ 'input.amount': { $gte: 500 } })
 * f.matches({ input: { amount: 750 } }) // true
 * ```
 */
export function compileFilter(src: unknown, options?: CompileOptions): CompiledFilter {
  const { pred, roots, json, matchAll } = compile(src, options?.trusted === true)
  return {
    source: json,
    matchAll,
    roots: roots as ReadonlySet<Exclude<Root, '@'>>,
    matches: (doc: FilterDoc) => {
      // The one place host exceptions end: every delivery mode answers the same.
      try {
        return pred(doc !== null && typeof doc === 'object' ? doc : {}, undefined)
      } catch {
        return false
      }
    },
  }
}

/** The filter that matches every document (`{}`). */
export const MATCH_ALL: CompiledFilter = compileFilter({})
