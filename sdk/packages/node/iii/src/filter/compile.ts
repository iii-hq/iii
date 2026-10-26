/**
 * Validation (filters.md §2, §4) and compilation into closures, in one pass.
 *
 * Mirrors `crates/iii-filter` (`parse.rs`, `eval.rs`): the same limits, the
 * same error code (`INVALID_FILTER`) and the same semantics, so a filter
 * evaluated here, in the engine or in another SDK answers the same. The
 * filter must already be JSON (`checkJson`); it is then copied through
 * `JSON.stringify` (also what the encoded size limit is measured on), so
 * what is validated is exactly what travels on the wire.
 *
 * Limits, as the vectors define them:
 * - depth: the top-level filter is at depth 1; the filters of a `$or`, the
 *   operand of `$not` and the operand of `$any` are one level deeper than the
 *   object holding them; the filters of a `$and` stay at the same level;
 * - nodes: every operator (`$eq` ... `$any`) and every `$or` / `$not` counts
 *   one; paths and `$and` are free;
 * - encoded size: the compact JSON of the filter as given, in UTF-8 bytes.
 *
 * Pure TypeScript with no Node.js APIs: the browser SDK uses this file too.
 */
import { FilterError, MAX_DEPTH, MAX_ENCODED_BYTES, MAX_LITERAL_DEPTH, MAX_NODES, MAX_SET_ITEMS } from './errors'
import { checkJson, deepFreeze } from './json'
import { compilePath, type FilterDoc, parsePath, type Root } from './path'
import { arrayElement, compareCodePoints, hasCodePointPrefix, type JsonView, jsonEquals, utf8Length } from './value'

/** A compiled filter node: `element` is the current `$any` element (`@`). */
type Pred = (doc: FilterDoc, element: JsonView) => boolean
/** A value operator: `value` is `undefined` when the path is absent. */
type OpFn = (value: JsonView, doc: FilterDoc) => boolean

type JsonObject = Record<string, unknown>

const VALUE_OPERATORS: ReadonlySet<string> = new Set([
  '$eq',
  '$ne',
  '$in',
  '$nin',
  '$gt',
  '$gte',
  '$lt',
  '$lte',
  '$exists',
  '$prefix',
  '$any',
])

class Budget {
  private used = 0

  take(): void {
    this.used++
    if (this.used > MAX_NODES) {
      throw FilterError.invalid(`more than ${MAX_NODES} operator / clause nodes`)
    }
  }
}

function isJsonObject(v: unknown): v is JsonObject {
  return v !== null && typeof v === 'object' && !Array.isArray(v)
}

function kind(v: unknown): string {
  if (v === null) return 'null'
  if (Array.isArray(v)) return 'an array'
  switch (typeof v) {
    case 'boolean':
      return 'a boolean'
    case 'number':
      return 'a number'
    case 'string':
      return 'a string'
    default:
      return 'an object'
  }
}

/**
 * Validates `src` and compiles it. Throws {@link FilterError} with code
 * `INVALID_FILTER` when the filter is not valid.
 *
 * `trusted` skips the encoded size limit, for filters the engine already
 * validated (the normal form it pushes to owners): normalizing can make a
 * filter a little longer than the one written, and JavaScript spells some
 * numbers longer than serde_json, so near the limit the two measurements can
 * disagree, and the engine's is the one that counts.
 */
export function compile(
  src: unknown,
  trusted = false,
): { pred: Pred; roots: Set<Root>; json: JsonObject; matchAll: boolean } {
  let text: string
  try {
    if (src === null || typeof src !== 'object' || Array.isArray(src)) {
      throw FilterError.invalid(`a filter must be a JSON object, found ${src === undefined ? 'nothing' : kind(src)}`)
    }
    checkJson(src)
    text = JSON.stringify(src)
  } catch (error) {
    if (error instanceof FilterError) throw error
    // a Proxy trap or a revoked proxy
    throw FilterError.invalid(`the filter cannot be read: ${error instanceof Error ? error.message : String(error)}`)
  }
  if (!trusted && utf8Length(text) > MAX_ENCODED_BYTES) {
    throw FilterError.invalid(`the filter encodes to more than ${MAX_ENCODED_BYTES} bytes`)
  }
  // Frozen: the compiled closures hold its operands, and `source` hands it out.
  const json = deepFreeze(JSON.parse(text) as JsonObject)
  const roots = new Set<Root>()
  const clauses: Pred[] = []
  collectClauses(json, 1, false, new Budget(), roots, clauses)
  return { pred: and(clauses), roots, json, matchAll: clauses.length === 0 }
}

function parseFilter(src: unknown, depth: number, inAny: boolean, budget: Budget, roots: Set<Root>): Pred {
  const clauses: Pred[] = []
  collectClauses(src, depth, inAny, budget, roots, clauses)
  return and(clauses)
}

function and(clauses: Pred[]): Pred {
  switch (clauses.length) {
    case 0:
      return () => true
    case 1:
      return clauses[0]
    case 2: {
      const [a, b] = clauses
      return (doc, el) => a(doc, el) && b(doc, el)
    }
    default:
      return (doc, el) => {
        for (let i = 0; i < clauses.length; i++) {
          if (!clauses[i](doc, el)) return false
        }
        return true
      }
  }
}

/**
 * Collects the clauses of `src` and of every filter its `$and`s hold. `$and`
 * adds no depth, so it is walked with an explicit stack.
 */
function collectClauses(
  src: unknown,
  depth: number,
  inAny: boolean,
  budget: Budget,
  roots: Set<Root>,
  out: Pred[],
): void {
  if (depth > MAX_DEPTH) {
    throw FilterError.invalid(`filters nested deeper than ${MAX_DEPTH} levels`)
  }
  const pending: unknown[] = [src]
  while (pending.length > 0) {
    const current = pending.pop()
    if (!isJsonObject(current)) {
      throw FilterError.invalid(`a filter must be a JSON object, found ${kind(current)}`)
    }
    for (const key of Object.keys(current)) {
      const value = current[key]
      if (key === '$and') {
        const items = filterList('$and', value)
        for (let i = items.length - 1; i >= 0; i--) pending.push(items[i])
      } else if (key === '$or') {
        budget.take()
        const children = filterList('$or', value).map((item) => parseFilter(item, depth + 1, inAny, budget, roots))
        out.push((doc, el) => {
          for (let i = 0; i < children.length; i++) {
            if (children[i](doc, el)) return true
          }
          return false
        })
      } else if (key === '$not') {
        budget.take()
        if (!isJsonObject(value)) {
          throw FilterError.invalid(`\`$not\` takes a filter object, found ${kind(value)}`)
        }
        const child = parseFilter(value, depth + 1, inAny, budget, roots)
        out.push((doc, el) => !child(doc, el))
      } else if (VALUE_OPERATORS.has(key)) {
        throw FilterError.invalid(`\`${key}\` needs a path: write {"<path>": {"${key}": ...}}`)
      } else if (key.startsWith('$')) {
        throw FilterError.invalid(`unknown operator \`${key}\` (a filter holds paths, \`$and\`, \`$or\` and \`$not\`)`)
      } else {
        const path = parsePath(key, inAny)
        if (path.root !== '@') roots.add(path.root)
        const ops = parseOperatorObject(key, value, depth, budget)
        const resolve = compilePath(path)
        out.push(pathClause(resolve, ops))
      }
    }
  }
}

function pathClause(resolve: (doc: FilterDoc, element: JsonView) => JsonView, ops: OpFn[]): Pred {
  if (ops.length === 1) {
    const op = ops[0]
    return (doc, el) => op(resolve(doc, el), doc)
  }
  return (doc, el) => {
    const value = resolve(doc, el)
    for (let i = 0; i < ops.length; i++) {
      if (!ops[i](value, doc)) return false
    }
    return true
  }
}

function filterList(name: string, value: unknown): unknown[] {
  if (!Array.isArray(value)) {
    throw FilterError.invalid(`\`${name}\` takes an array of filters, found ${kind(value)}`)
  }
  if (value.length === 0) {
    throw FilterError.invalid(`\`${name}\` takes a non-empty array of filters`)
  }
  return value
}

function parseOperatorObject(path: string, value: unknown, depth: number, budget: Budget): OpFn[] {
  const shorthand = () =>
    FilterError.invalid(`\`${path}\`: expected an operator object such as {"$eq": ...}; literal values are not allowed`)
  if (!isJsonObject(value)) throw shorthand()
  const names = Object.keys(value)
  if (names.length === 0) throw FilterError.invalid(`\`${path}\`: empty operator object`)
  if (names.some((k) => !k.startsWith('$'))) throw shorthand()
  return parseOperators(path, value, depth, budget)
}

/** The operators of an operator object; `depth` is the level of the object holding them. */
function parseOperators(path: string, map: JsonObject, depth: number, budget: Budget): OpFn[] {
  return Object.keys(map).map((name) => {
    budget.take()
    return parseOperator(path, name, map[name], depth, budget)
  })
}

function parseOperator(path: string, name: string, operand: unknown, depth: number, budget: Budget): OpFn {
  const wrong = (expected: string) =>
    FilterError.invalid(`\`${path}\`: \`${name}\` takes ${expected}, found ${kind(operand)}`)
  switch (name) {
    // Numbers anywhere in the filter were checked against ±2^53 by `checkJson`.
    case '$eq':
      checkLiteralDepth(path, name, operand)
      return (v) => v !== undefined && jsonEquals(v, operand)
    case '$ne':
      checkLiteralDepth(path, name, operand)
      return (v) => v === undefined || !jsonEquals(v, operand)
    case '$in':
    case '$nin': {
      if (!Array.isArray(operand)) throw wrong('an array')
      if (operand.length > MAX_SET_ITEMS) {
        throw FilterError.invalid(`\`${path}\`: \`${name}\` has ${operand.length} items (limit ${MAX_SET_ITEMS})`)
      }
      for (const item of operand) checkLiteralDepth(path, name, item)
      const items = operand
      const member = (v: JsonView) => {
        for (let i = 0; i < items.length; i++) {
          if (jsonEquals(v, items[i])) return true
        }
        return false
      }
      return name === '$in' ? (v) => v !== undefined && member(v) : (v) => v === undefined || !member(v)
    }
    case '$gt':
    case '$gte':
    case '$lt':
    case '$lte': {
      const accept: (c: number) => boolean =
        name === '$gt' ? (c) => c > 0 : name === '$gte' ? (c) => c >= 0 : name === '$lt' ? (c) => c < 0 : (c) => c <= 0
      if (typeof operand === 'number') {
        const bound = operand
        return (v) => typeof v === 'number' && accept(v < bound ? -1 : v > bound ? 1 : 0)
      }
      if (typeof operand === 'string') {
        const bound = operand
        return (v) => typeof v === 'string' && accept(compareCodePoints(v, bound))
      }
      throw wrong('a number or a string')
    }
    case '$exists': {
      if (typeof operand !== 'boolean') throw wrong('a boolean')
      const expected = operand
      return (v) => (v !== undefined) === expected
    }
    case '$prefix': {
      if (typeof operand !== 'string') throw wrong('a string')
      const prefix = operand
      return (v) => typeof v === 'string' && hasCodePointPrefix(v, prefix)
    }
    case '$any':
      return parseAny(path, operand, depth, budget)
    case '$and':
    case '$or':
    case '$not':
      throw FilterError.invalid(`\`${path}\`: \`${name}\` combines filters and is not a value operator`)
    default:
      throw FilterError.invalid(`\`${path}\`: unknown operator \`${name}\``)
  }
}

/**
 * `$any` takes an operator object (all keys are value operators) or a filter
 * over the element (`@` paths, `$and`, `$or`, `$not`). `{}` is the empty
 * filter: it matches any non-empty array.
 */
function parseAny(path: string, operand: unknown, depth: number, budget: Budget): OpFn {
  if (!isJsonObject(operand)) {
    throw FilterError.invalid(
      `\`${path}\`: \`$any\` takes an operator object or a filter over \`@\`, found ${kind(operand)}`,
    )
  }
  if (depth + 1 > MAX_DEPTH) {
    throw FilterError.invalid(`filters nested deeper than ${MAX_DEPTH} levels`)
  }
  const names = Object.keys(operand)
  if (names.length > 0 && names.every((k) => VALUE_OPERATORS.has(k))) {
    const ops = parseOperators(path, operand, depth + 1, budget)
    return (v, doc) => {
      if (!Array.isArray(v)) return false
      for (let i = 0; i < v.length; i++) {
        const item = arrayElement(v, i)
        let all = true
        for (let j = 0; j < ops.length; j++) {
          if (!ops[j](item, doc)) {
            all = false
            break
          }
        }
        if (all) return true
      }
      return false
    }
  }
  // `@` paths add no root: the `$any` clause's own path names it.
  const node = parseFilter(operand, depth + 1, true, budget, new Set())
  return (v, doc) => {
    if (!Array.isArray(v)) return false
    for (let i = 0; i < v.length; i++) {
      if (node(doc, arrayElement(v, i))) return true
    }
    return false
  }
}

/**
 * A literal (an `$eq` / `$ne` operand, an `$in` / `$nin` item) nests at most
 * MAX_LITERAL_DEPTH arrays / objects: a scalar is 0, `[]` and `[1]` are 1.
 */
function checkLiteralDepth(path: string, name: string, literal: unknown): void {
  const stack: [unknown, number][] = [[literal, 1]]
  while (stack.length > 0) {
    const [value, level] = stack.pop() as [unknown, number]
    if (value === null || typeof value !== 'object') continue
    if (level > MAX_LITERAL_DEPTH) {
      throw FilterError.invalid(
        `\`${path}\`: \`${name}\` nests a literal deeper than ${MAX_LITERAL_DEPTH} arrays / objects`,
      )
    }
    for (const member of Object.values(value as Record<string, unknown>)) stack.push([member, level + 1])
  }
}
