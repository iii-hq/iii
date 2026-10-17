/**
 * The filter as JSON. A filter written in JavaScript must already be JSON:
 * `JSON.stringify` would silently drop or rewrite what is not (an `undefined`
 * clause disappears, a `Map` or a class instance becomes `{}`, `NaN` becomes
 * `null`), and a dropped clause turns a filter into one that matches more.
 * So every value is checked first, and anything JSON would not carry as it is
 * is `INVALID_FILTER`, naming where it is.
 *
 * Pure TypeScript with no Node.js APIs: the browser SDK uses this file too.
 */
import { FilterError, MAX_ENCODED_BYTES, MAX_JSON_DEPTH, MAX_SAFE_LITERAL } from './errors'

/** A value to check, or (`leave`) the end of an object / array, which then stops being an ancestor. */
type Pending = { value: unknown; at: string; depth: number; leave?: true }

function where(at: string): string {
  return at === '' ? 'the filter' : `\`${at}\``
}

/** A string serde_json would refuse (a lone surrogate) is refused here too. */
function checkString(s: string, at: string, what: string): void {
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i)
    if (c < 0xd800 || c > 0xdfff) continue
    const next = s.charCodeAt(i + 1)
    if (c <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) {
      i++
      continue
    }
    throw FilterError.invalid(`${where(at)}: ${what} holds a lone UTF-16 surrogate, which is not valid Unicode`)
  }
}

/** An object JSON writes member by member: a prototype-less object or one whose prototype is a realm's `Object.prototype`. */
function isPlainObject(v: object): boolean {
  const proto = Object.getPrototypeOf(v)
  return proto === null || Object.getPrototypeOf(proto) === null
}

function describe(v: unknown): string {
  if (typeof v === 'number') return String(v)
  if (typeof v !== 'object' || v === null) return typeof v
  const name = (Object.getPrototypeOf(v) as { constructor?: { name?: unknown } } | null)?.constructor?.name
  return typeof name === 'string' && name !== '' ? `a ${name}` : 'an object'
}

/**
 * Checks that `src` is JSON as it stands: plain objects (own enumerable data
 * properties, string keys) and dense arrays of strings, finite numbers within
 * ±2^53, booleans and `null`, without cycles (a value shared by two members
 * is fine), nested at most MAX_JSON_DEPTH arrays / objects. Iterative, and
 * bounded by the encoded size limit (every JSON value takes at least one
 * byte, so a value shared many times over is refused quickly).
 */
export function checkJson(src: unknown): void {
  const stack: Pending[] = [{ value: src, at: '', depth: 1 }]
  const ancestors = new Set<object>()
  let visited = 0
  while (stack.length > 0) {
    const { value, at, depth, leave } = stack.pop() as Pending
    if (leave) {
      ancestors.delete(value as object)
      continue
    }
    if (++visited > MAX_ENCODED_BYTES) {
      throw FilterError.invalid(`the filter encodes to more than ${MAX_ENCODED_BYTES} bytes`)
    }
    switch (typeof value) {
      case 'boolean':
        continue
      case 'string':
        checkString(value, at, 'a string')
        continue
      case 'number':
        if (!Number.isFinite(value)) {
          throw FilterError.invalid(`${where(at)}: ${value} is not a JSON number`)
        }
        // |n| <= 2^53, so JavaScript and Rust read every literal alike.
        if (Math.abs(value) > MAX_SAFE_LITERAL) {
          throw FilterError.invalid(`${where(at)}: the number ${value} is outside ±2^53`)
        }
        continue
      case 'object': {
        if (value === null) continue
        if (ancestors.has(value)) {
          throw FilterError.invalid(`${where(at)}: a cycle (the value contains itself)`)
        }
        if (depth > MAX_JSON_DEPTH) {
          throw FilterError.invalid(`${where(at)}: the filter nests deeper than ${MAX_JSON_DEPTH} arrays / objects`)
        }
        ancestors.add(value)
        stack.push({ value, at, depth, leave: true })
        if (Array.isArray(value)) {
          for (let i = 0; i < value.length; i++) {
            if (!(i in value)) throw FilterError.invalid(`${where(at)}: the array has a hole at ${i}`)
            stack.push({ value: value[i], at: `${at}[${i}]`, depth: depth + 1 })
          }
          continue
        }
        if (!isPlainObject(value)) {
          throw FilterError.invalid(`${where(at)}: ${describe(value)} is not a JSON object (use a plain object)`)
        }
        if (Object.getOwnPropertySymbols(value).length > 0) {
          throw FilterError.invalid(`${where(at)}: symbol keys are not JSON`)
        }
        for (const key of Object.getOwnPropertyNames(value)) {
          const member = at === '' ? key : `${at}.${key}`
          const descriptor = Object.getOwnPropertyDescriptor(value, key)
          if (!descriptor?.enumerable) {
            throw FilterError.invalid(`${where(member)}: a non-enumerable property is not JSON`)
          }
          if (!('value' in descriptor)) {
            throw FilterError.invalid(`${where(member)}: an accessor property is not JSON`)
          }
          checkString(key, member, 'the key')
          stack.push({ value: descriptor.value, at: member, depth: depth + 1 })
        }
        continue
      }
      default:
        // undefined, functions, symbols, BigInt
        throw FilterError.invalid(`${where(at)}: ${describe(value)} is not JSON`)
    }
  }
}

/** Freezes a parsed JSON value (iteratively): the closures compiled from it share its operands. */
export function deepFreeze<T>(value: T): T {
  const stack: unknown[] = [value]
  while (stack.length > 0) {
    const v = stack.pop()
    if (v === null || typeof v !== 'object' || Object.isFrozen(v)) continue
    Object.freeze(v)
    for (const member of Object.values(v as Record<string, unknown>)) stack.push(member)
  }
  return value
}
