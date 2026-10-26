/**
 * Host values seen through JSON (filters.md §3): the evaluator answers as if
 * the document were `JSON.parse(JSON.stringify(doc))`, without serializing it.
 * Every value is mapped lazily, one step at a time, when a path reaches it.
 *
 * Pure TypeScript with no Node.js APIs: the browser SDK uses this file too.
 */

/** A value as JSON sees it: `undefined` is absent, everything else is JSON-shaped. */
export type JsonView = null | boolean | number | string | readonly unknown[] | Record<string, unknown> | undefined

const hasEnumerable = Object.prototype.propertyIsEnumerable
const toStringTag = Object.prototype.toString

/** A `BigInt` read through its JSON encoding (its decimal digits) as a double; beyond the double range, `null`. */
function bigintView(b: bigint): number | null {
  const n = Number(b)
  return Number.isFinite(n) ? n : null
}

/**
 * Unwraps a boxed primitive (`new Number(1)`), also one from another realm,
 * by its internal slot rather than `instanceof`. Anything else is an object.
 */
function unbox(v: object): JsonView {
  try {
    switch (toStringTag.call(v)) {
      case '[object Number]': {
        const n = Number.prototype.valueOf.call(v)
        return Number.isFinite(n) ? n : null
      }
      case '[object String]':
        return String.prototype.valueOf.call(v)
      case '[object Boolean]':
        return Boolean.prototype.valueOf.call(v)
      case '[object BigInt]':
        return bigintView(BigInt.prototype.valueOf.call(v))
    }
  } catch {
    // a spoofed Symbol.toStringTag: an ordinary object
  }
  return v as Record<string, unknown>
}

/**
 * One step of `JSON.stringify` (SerializeJSONProperty): `toJSON`, boxed
 * primitives, non-finite numbers, and the values JSON drops. `BigInt` is read
 * through its JSON encoding (its decimal digits), as a double, unless the
 * program installed `BigInt.prototype.toJSON`. Objects and arrays are
 * returned as they are; their members are mapped when a path steps into them.
 *
 * Host code runs here (`toJSON`, getters, Proxy traps) and may throw: the
 * caller of the whole evaluation turns that into a non-match.
 */
export function jsonView(value: unknown, key: string): JsonView {
  let v = value
  if (v !== null && (typeof v === 'object' || typeof v === 'function' || typeof v === 'bigint')) {
    const toJSON = (v as { toJSON?: unknown }).toJSON
    if (typeof toJSON === 'function') v = toJSON.call(v, key)
  }
  switch (typeof v) {
    case 'string':
    case 'boolean':
      return v
    case 'number':
      return Number.isFinite(v) ? v : null
    case 'bigint':
      return bigintView(v)
    case 'object': {
      if (v === null) return null
      if (Array.isArray(v)) return v
      const proto = Object.getPrototypeOf(v)
      if (proto === Object.prototype || proto === null) return v as Record<string, unknown>
      return unbox(v)
    }
    default:
      // undefined, functions and symbols: JSON leaves them out
      return undefined
  }
}

/** Element `index` of an array, as JSON writes it: a member JSON drops is `null`. */
export function arrayElement(array: readonly unknown[], index: number): JsonView {
  const v = jsonView(array[index], String(index))
  return v === undefined ? null : v
}

/** Own enumerable key `key` of an object; inherited properties do not exist. */
export function objectMember(object: Record<string, unknown>, key: string): JsonView {
  if (!hasEnumerable.call(object, key)) return undefined
  return jsonView(object[key], key)
}

export function isArray(v: JsonView): v is readonly unknown[] {
  return Array.isArray(v)
}

export function isObject(v: JsonView): v is Record<string, unknown> {
  return v !== null && typeof v === 'object' && !Array.isArray(v)
}

/**
 * JSON deep equality between a mapped host value and a filter literal (plain
 * JSON). Numbers compare as doubles (`1 == 1.0`, `-0 == 0`), object key order
 * is ignored, array order is not, and there is no coercion between types.
 * Iterative, so a deeply nested literal cannot overflow the stack.
 */
export function jsonEquals(host: JsonView, literal: unknown): boolean {
  if (literal === null || typeof literal !== 'object') return scalarEquals(host, literal)
  const stack: [JsonView, unknown][] = [[host, literal]]
  while (stack.length > 0) {
    const [h, l] = stack.pop() as [JsonView, unknown]
    if (l === null || typeof l !== 'object') {
      if (!scalarEquals(h, l)) return false
      continue
    }
    if (Array.isArray(l)) {
      if (!Array.isArray(h) || h.length !== l.length) return false
      for (let i = 0; i < l.length; i++) stack.push([arrayElement(h, i), l[i]])
      continue
    }
    if (!isObject(h)) return false
    const lit = l as Record<string, unknown>
    const keys = Object.keys(lit)
    for (const k of keys) {
      const member = objectMember(h, k)
      if (member === undefined) return false
      stack.push([member, lit[k]])
    }
    // the host must not hold more members than the literal
    let present = 0
    for (const k of Object.keys(h)) {
      if (jsonView(h[k], k) !== undefined) {
        present++
        if (present > keys.length) return false
      }
    }
    if (present !== keys.length) return false
  }
  return true
}

function scalarEquals(host: JsonView, literal: unknown): boolean {
  if (literal === null) return host === null
  switch (typeof literal) {
    case 'number':
    case 'string':
    case 'boolean':
      return host === literal
    default:
      return false
  }
}

/**
 * Compares two strings by Unicode code point (UTF-8 byte order), not by
 * UTF-16 code unit as `<` does: a surrogate pair (a code point above U+FFFF)
 * sorts after every BMP character, including U+E000..U+FFFF.
 */
export function compareCodePoints(a: string, b: string): number {
  if (a === b) return 0
  const n = a.length < b.length ? a.length : b.length
  for (let i = 0; i < n; i++) {
    let ca = a.charCodeAt(i)
    let cb = b.charCodeAt(i)
    if (ca !== cb) {
      if (ca >= 0xd800 && cb >= 0xd800) {
        // fix up the only range where code unit and code point order differ
        ca = ca >= 0xe000 ? ca - 0x800 : ca + 0x2000
        cb = cb >= 0xe000 ? cb - 0x800 : cb + 0x2000
      }
      return ca < cb ? -1 : 1
    }
  }
  return a.length < b.length ? -1 : a.length > b.length ? 1 : 0
}

/** `$prefix`: code point by code point, so a prefix never ends inside a surrogate pair. */
export function hasCodePointPrefix(s: string, prefix: string): boolean {
  if (!s.startsWith(prefix)) return false
  if (prefix.length === 0 || prefix.length === s.length) return true
  const last = prefix.charCodeAt(prefix.length - 1)
  if (last < 0xd800 || last > 0xdbff) return true
  const next = s.charCodeAt(prefix.length)
  return next < 0xdc00 || next > 0xdfff
}

/** UTF-8 byte length of a string, without `Buffer` or `TextEncoder`. */
export function utf8Length(s: string): number {
  let bytes = 0
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i)
    if (c < 0x80) bytes += 1
    else if (c < 0x800) bytes += 2
    else if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
      const d = s.charCodeAt(i + 1)
      if (d >= 0xdc00 && d <= 0xdfff) {
        bytes += 4
        i++
      } else {
        bytes += 3
      }
    } else bytes += 3
  }
  return bytes
}
