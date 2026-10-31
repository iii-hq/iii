/**
 * Many filters matched against one document by an index of their equality
 * conditions (decision 55), by the rule of the engine's
 * `iii_filter::FilterIndex`.
 *
 * A filter matches a document only if every clause of its top-level `$and`
 * does. A clause on a path with `$eq` of a scalar (or `$in` of scalars)
 * therefore names the values the document's value at that path must equal.
 * {@link FilterIndex} files each filter under one such condition, path to
 * value to entries, and keeps the filters without one in a list it scans.
 * Matching a document resolves each indexed path once, looks its value up,
 * and evaluates the whole filter of only the entries found there and of the
 * scanned ones: about one lookup per indexed path plus one evaluation per
 * candidate, however many filters the index holds.
 *
 * The index rules out only entries whose condition the document fails, and
 * evaluates every other entry's filter in full, so its matches are exactly
 * the entries that evaluating every filter would yield, in the order of their
 * keys.
 *
 * Values are compared by the filter language's equality, which a `Map`
 * already keys by: numbers by value (`1` and `1.0` are one number, `-0` is
 * `0`), strings by their characters, `null` and booleans as themselves, and a
 * value of one type never equals a value of another. An array or object
 * operand is never indexed: its clause is evaluated with the rest of the
 * filter.
 *
 * Pure TypeScript with no Node.js APIs: the browser SDK uses this file too.
 */
import type { CompiledFilter } from './index'
import { compilePath, type FilterDoc, parsePath } from './path'
import type { JsonView } from './value'

/** A value an equality condition is indexed by. */
type Scalar = null | boolean | number | string

function isScalar(value: unknown): value is Scalar {
  return value === null || typeof value === 'boolean' || typeof value === 'number' || typeof value === 'string'
}

/** One `$eq` / `$in` condition of a filter's top-level `$and`: the path must equal one of `values`. */
type Condition = { path: string; raw: string; values: Scalar[] }

/**
 * The equality conditions of `source`, a filter that compiled: for each
 * clause of its top-level `$and` (its keys and those of the filters its
 * `$and`s hold, never inside `$or`, `$not` or `$any`), its `$eq` operand or
 * its `$in` operands, when they are scalars. `path` is the parsed path,
 * spelled one way whatever escapes the filter used.
 */
function conditionsOf(source: Record<string, unknown>): Condition[] {
  const out: Condition[] = []
  const pending: unknown[] = [source]
  while (pending.length > 0) {
    const current = pending.pop() as Record<string, unknown>
    for (const key of Object.keys(current)) {
      const value = current[key]
      if (key === '$and') {
        for (const item of value as unknown[]) pending.push(item)
        continue
      }
      if (key.startsWith('$')) continue
      const ops = value as Record<string, unknown>
      const eq = ops.$eq
      const inItems = ops.$in
      const hasEq = Object.prototype.hasOwnProperty.call(ops, '$eq')
      if (!hasEq && !Array.isArray(inItems)) continue
      const parsed = parsePath(key, false)
      const path = JSON.stringify([parsed.root, ...parsed.segments.map((s) => s.key)])
      if (hasEq && isScalar(eq)) out.push({ path, raw: key, values: [eq] })
      if (Array.isArray(inItems) && inItems.length > 0 && inItems.every(isScalar)) {
        out.push({ path, raw: key, values: [...new Set(inItems as Scalar[])] })
      }
    }
  }
  return out
}

type Entry<T> = { value: T; filter: CompiledFilter; order: number }

/** The entries filed under one path, by value. */
type PathIndex<T> = {
  resolve: (doc: FilterDoc, element: JsonView) => JsonView
  buckets: Map<Scalar, Set<Entry<T>>>
  /** Entries filed here. */
  entries: number
}

/** Where an entry is filed: under each of `values` of `path`, or (`undefined`) in the scanned list. */
type Filed<T> = { entry: Entry<T>; path?: string; values?: Scalar[] }

/**
 * Filters, each with a value and an order, matched against a document by an
 * index of their equality conditions (see the module docs).
 * {@link FilterIndex.matches} returns exactly the values whose filter
 * matches the document (and that `accept` accepts), by ascending `order`.
 */
export class FilterIndex<T> {
  private readonly filed = new Map<T, Filed<T>>()
  /** Entries without a condition the index serves. */
  private readonly scanned = new Set<Entry<T>>()
  private readonly paths = new Map<string, PathIndex<T>>()
  /** Entries filed under an indexed path. */
  private indexed = 0

  /** Entries held. */
  get size(): number {
    return this.filed.size
  }

  /**
   * Whether {@link FilterIndex.matches} can evaluate fewer filters than
   * evaluating each entry's would. Looking a document's value up at an
   * indexed path costs about one evaluation, and rules out the entries filed
   * there under other values, so it can only when some indexed path files
   * more than one entry. Otherwise evaluating each filter in order costs less
   * and yields the same matches.
   */
  get narrows(): boolean {
    return this.indexed > this.paths.size
  }

  has(value: T): boolean {
    return this.filed.has(value)
  }

  /**
   * Files `value` with `filter`, in place of the entry `value` had. `order`
   * sorts the matches. The condition it is filed under is the one of the
   * filter's top-level `$and` that the fewest entries share now (an `$eq`
   * before an `$in` of several values).
   */
  insert(value: T, filter: CompiledFilter, order: number): void {
    this.remove(value)
    const entry: Entry<T> = { value, filter, order }
    const condition = this.conditionFor(filter)
    if (!condition) {
      this.scanned.add(entry)
      this.filed.set(value, { entry })
      return
    }
    let index = this.paths.get(condition.path)
    if (!index) {
      index = { resolve: compilePath(parsePath(condition.raw, false)), buckets: new Map(), entries: 0 }
      this.paths.set(condition.path, index)
    }
    for (const scalar of condition.values) {
      const bucket = index.buckets.get(scalar)
      if (bucket) bucket.add(entry)
      else index.buckets.set(scalar, new Set([entry]))
    }
    index.entries++
    this.indexed++
    this.filed.set(value, { entry, path: condition.path, values: condition.values })
  }

  /** Takes the entry of `value` out. */
  remove(value: T): void {
    const filed = this.filed.get(value)
    if (!filed) return
    this.filed.delete(value)
    if (filed.path === undefined || filed.values === undefined) {
      this.scanned.delete(filed.entry)
      return
    }
    const index = this.paths.get(filed.path)
    if (!index) return
    this.indexed--
    for (const scalar of filed.values) {
      const bucket = index.buckets.get(scalar)
      if (!bucket) continue
      bucket.delete(filed.entry)
      if (bucket.size === 0) index.buckets.delete(scalar)
    }
    if (--index.entries === 0) this.paths.delete(filed.path)
  }

  /**
   * The values whose filter matches `doc`, by ascending order. Resolves each
   * indexed path of `doc` once, and evaluates the filters of the entries
   * filed under the value found there and of the scanned entries
   * ({@link FilterIndex.candidates} counts them); every other entry has a
   * condition `doc` fails. `accept` is asked first, for each candidate.
   */
  matches(doc: FilterDoc, accept?: (value: T) => boolean): T[] {
    const found: Entry<T>[] = []
    // Sorted only when found out of order.
    let sorted = true
    let last = -Infinity
    if (this.paths.size > 0) {
      for (const index of this.paths.values()) {
        const bucket = this.bucket(index, doc)
        if (bucket === undefined) continue
        for (const entry of bucket) {
          if ((accept === undefined || accept(entry.value)) && entry.filter.matches(doc)) {
            if (entry.order < last) sorted = false
            last = entry.order
            found.push(entry)
          }
        }
      }
    }
    if (this.scanned.size > 0) {
      for (const entry of this.scanned) {
        if ((accept === undefined || accept(entry.value)) && entry.filter.matches(doc)) {
          if (entry.order < last) sorted = false
          last = entry.order
          found.push(entry)
        }
      }
    }
    if (!sorted) found.sort((a, b) => a.order - b.order)
    const values = new Array<T>(found.length)
    for (let i = 0; i < found.length; i++) values[i] = found[i].value
    return values
  }

  /** How many filters a match of `doc` evaluates: the entries filed under its values, and the scanned ones. */
  candidates(doc: FilterDoc): number {
    let n = this.scanned.size
    for (const index of this.paths.values()) n += this.bucket(index, doc)?.size ?? 0
    return n
  }

  /**
   * The entries filed under `doc`'s value at the path. A value an index
   * cannot hold (absent, an array, an object) matches no condition, and
   * neither does a path whose reading throws: each filter filed under it
   * holds a clause on that path, and a filter whose reading throws does not
   * match.
   */
  private bucket(index: PathIndex<T>, doc: FilterDoc): Set<Entry<T>> | undefined {
    let value: JsonView
    try {
      value = index.resolve(doc !== null && typeof doc === 'object' ? doc : {}, undefined)
    } catch {
      return undefined
    }
    return isScalar(value) ? index.buckets.get(value) : undefined
  }

  private conditionFor(filter: CompiledFilter): Condition | undefined {
    let best: { condition: Condition; score: [number, number, number] } | undefined
    for (const condition of conditionsOf(filter.source)) {
      const held = this.paths.get(condition.path)
      let sharing = 0
      if (held) for (const scalar of condition.values) sharing += held.buckets.get(scalar)?.size ?? 0
      const score: [number, number, number] = [condition.values.length > 1 ? 1 : 0, sharing, condition.values.length]
      if (best === undefined || lessThan(score, best.score)) best = { condition, score }
    }
    return best?.condition
  }
}

function lessThan(a: readonly number[], b: readonly number[]): boolean {
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return a[i] < b[i]
  }
  return false
}
