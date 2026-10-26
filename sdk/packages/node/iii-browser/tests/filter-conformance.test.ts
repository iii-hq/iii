// Derived from sdk/packages/node/iii/tests/filter-conformance.test.ts by scripts/sync-browser-observe.py; edit those, not this file.
import { readdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { runInNewContext } from 'node:vm'
import { describe, expect, it } from 'vitest'
import * as nodeFilter from '../../iii/src/filter'
import { compileFilter, FilterError } from '../src/filter'

/**
 * Filter conformance (sdk.md §7): every shared vector in
 * `sdk/fixtures/filters`, plus the differential corpus produced by the Rust
 * evaluator (`crates/iii-filter`, `diffgen`). A file is never skipped.
 */
const FIXTURES = resolve(__dirname, '../../../../fixtures/filters')

type Doc = Record<string, unknown>
type ValidVector = { description?: string; filters: unknown; cases: { doc: Doc; expect: boolean; note?: string }[] }
type InvalidVector = { description?: string; cases: { filters: unknown; code: string; note?: string }[] }
type Corpus = { runs: { seed: number; cases: { filters: unknown; doc: Doc; expect: boolean }[] }[] }

function readJson<T>(path: string): T {
  return JSON.parse(readFileSync(path, 'utf8')) as T
}

function jsonFiles(dir: string): string[] {
  return readdirSync(dir)
    .filter((f) => f.endsWith('.json'))
    .sort()
}

/** The vector document as the SDK hands it to a filter: missing keys are absent roots. */
function asDoc(doc: Doc) {
  return {
    input: doc.input,
    output: doc.output,
    error: doc.error,
    caller: doc.caller,
    metadata: doc.metadata,
  }
}

function errorCode(fn: () => unknown): string | undefined {
  try {
    fn()
    return undefined
  } catch (error) {
    return error instanceof FilterError ? error.code : `not a FilterError: ${String(error)}`
  }
}

/**
 * The browser SDK imports the Node SDK's evaluator instead of copying it, so
 * this runs the same code the Node SDK's own conformance test runs, here under
 * the browser package's build settings and in a DOM environment.
 */
describe('the shared evaluator', () => {
  it('is the Node SDK module, not a copy', () => {
    expect(compileFilter).toBe(nodeFilter.compileFilter)
    expect(FilterError).toBe(nodeFilter.FilterError)
  })
})

describe('filter conformance vectors', () => {
  const valid = jsonFiles(FIXTURES)
  const invalid = jsonFiles(join(FIXTURES, 'invalid'))

  it('finds the vector files', () => {
    expect(valid.length).toBeGreaterThan(100)
    expect(invalid.length).toBeGreaterThan(10)
  })

  describe.each(valid)('valid %s', (file) => {
    const vector = readJson<ValidVector>(join(FIXTURES, file))

    it('accepts the filter and evaluates every case', () => {
      const filter = compileFilter(vector.filters)
      for (const [i, c] of vector.cases.entries()) {
        expect(filter.matches(asDoc(c.doc)), `case ${i}${c.note ? ` (${c.note})` : ''}`).toBe(c.expect)
      }
    })
  })

  describe.each(invalid)('invalid %s', (file) => {
    const vector = readJson<InvalidVector>(join(FIXTURES, 'invalid', file))

    it('rejects every filter with its code', () => {
      for (const [i, c] of vector.cases.entries()) {
        expect(
          errorCode(() => compileFilter(c.filters)),
          `case ${i}${c.note ? ` (${c.note})` : ''}`,
        ).toBe(c.code)
      }
    })
  })
})

describe('filter differential corpus (Rust diffgen)', () => {
  const corpus = readJson<Corpus>(join(FIXTURES, 'diff', 'corpus.json'))

  it('holds the committed runs', () => {
    expect(corpus.runs.length).toBeGreaterThan(0)
    expect(corpus.runs.reduce((n, r) => n + r.cases.length, 0)).toBeGreaterThanOrEqual(1000)
  })

  it.each(corpus.runs.map((r) => [r.seed, r] as const))('agrees with the Rust evaluator on seed %i', (_seed, run) => {
    const disagreements: string[] = []
    for (const [i, c] of run.cases.entries()) {
      let got: boolean | string
      try {
        got = compileFilter(c.filters).matches(asDoc(c.doc))
      } catch (error) {
        got = `rejected: ${error instanceof Error ? error.message : String(error)}`
      }
      if (got !== c.expect) {
        disagreements.push(
          `#${i}: expected ${c.expect}, got ${got}\n  filters ${JSON.stringify(c.filters)}\n  doc ${JSON.stringify(c.doc)}`,
        )
      }
    }
    expect(disagreements).toEqual([])
  })
})

describe('host values (filters.md §3)', () => {
  it('reads undefined as absent and drops it inside objects', () => {
    const f = compileFilter({ 'input.a': { $exists: false } })
    expect(f.matches({ input: { a: undefined } })).toBe(true)
    expect(f.matches({ input: undefined })).toBe(true)
    const eq = compileFilter({ input: { $eq: { b: 1 } } })
    expect(eq.matches({ input: { b: 1, c: undefined, d: () => 1 } })).toBe(true)
  })

  it('reads undefined, functions and symbols in arrays as null', () => {
    const f = compileFilter({ 'input.0': { $eq: null } })
    expect(f.matches({ input: [undefined] })).toBe(true)
    expect(f.matches({ input: [() => 1] })).toBe(true)
    expect(f.matches({ input: [Symbol('s')] })).toBe(true)
    // biome-ignore lint/suspicious/noSparseArray: a hole is what is tested
    expect(compileFilter({ input: { $eq: [1, null] } }).matches({ input: [1, ,] })).toBe(true)
  })

  it('reads a Date as its ISO string', () => {
    const f = compileFilter({ 'input.at': { $gte: '2026-11-01T00:00:00Z' } })
    expect(f.matches({ input: { at: new Date('2026-12-01T00:00:00Z') } })).toBe(true)
    expect(f.matches({ input: { at: new Date('2026-10-01T00:00:00Z') } })).toBe(false)
    expect(
      compileFilter({ 'input.at': { $eq: '2026-12-01T00:00:00.000Z' } }).matches({
        input: { at: new Date('2026-12-01T00:00:00Z') },
      }),
    ).toBe(true)
    expect(compileFilter({ 'input.at': { $eq: null } }).matches({ input: { at: new Date('nope') } })).toBe(true)
  })

  it('reads a BigInt through its JSON encoding (a number)', () => {
    expect(compileFilter({ 'input.n': { $eq: 42 } }).matches({ input: { n: 42n } })).toBe(true)
    expect(compileFilter({ 'input.n': { $gt: 9007199254740991 } }).matches({ input: { n: 2n ** 64n } })).toBe(true)
    expect(compileFilter({ 'input.n': { $eq: '42' } }).matches({ input: { n: 42n } })).toBe(false)
  })

  it('honours toJSON and unwraps boxed primitives', () => {
    class Money {
      constructor(private readonly cents: number) {}
      toJSON() {
        return { amount: this.cents / 100 }
      }
    }
    expect(compileFilter({ 'input.price.amount': { $eq: 12.5 } }).matches({ input: { price: new Money(1250) } })).toBe(
      true,
    )
    expect(compileFilter({ 'input.price.cents': { $exists: false } }).matches({ input: { price: new Money(1) } })).toBe(
      true,
    )
    expect(compileFilter({ 'input.s': { $prefix: 'ab' } }).matches({ input: { s: new String('abc') } })).toBe(true)
    expect(compileFilter({ 'input.n': { $eq: null } }).matches({ input: { n: Number.NaN } })).toBe(true)
  })

  it('sees own enumerable keys only', () => {
    class Row {
      visible = 1
      get inherited() {
        return 2
      }
    }
    expect(compileFilter({ 'input.inherited': { $exists: false } }).matches({ input: new Row() })).toBe(true)
    expect(compileFilter({ 'input.constructor': { $exists: false } }).matches({ input: new Row() })).toBe(true)
    expect(compileFilter({ 'input.length': { $exists: false } }).matches({ input: [1, 2] })).toBe(true)
    expect(compileFilter({ 'input.length': { $exists: false } }).matches({ input: 'abc' })).toBe(true)
    expect(compileFilter({ 'input.0': { $exists: false } }).matches({ input: 'abc' })).toBe(true)
    const withProto = JSON.parse('{"__proto__": {"x": 1}}')
    expect(compileFilter({ 'input.__proto__.x': { $eq: 1 } }).matches({ input: withProto })).toBe(true)
    expect(compileFilter({ 'input.__proto__': { $exists: false } }).matches({ input: {} })).toBe(true)
  })

  it('compares strings by code point, not by UTF-16 code unit', () => {
    expect(compileFilter({ 'input.s': { $gt: '｡' } }).matches({ input: { s: '😀' } })).toBe(true)
    expect(compileFilter({ 'input.s': { $lt: '😀' } }).matches({ input: { s: '｡' } })).toBe(true)
  })

  it('validates the filter as JSON', () => {
    expect(errorCode(() => compileFilter(null))).toBe('INVALID_FILTER')
    expect(errorCode(() => compileFilter([]))).toBe('INVALID_FILTER')
    expect(errorCode(() => compileFilter({ 'input.a': { $eq: 1n } }))).toBe('INVALID_FILTER')
    expect(errorCode(() => compileFilter({ 'input.a': { $eq: undefined } }))).toBe('INVALID_FILTER')
    expect(compileFilter({}).matchAll).toBe(true)
    expect(compileFilter({ $and: [{}] }).matchAll).toBe(true)
    expect(compileFilter({ 'input.a': { $eq: 1 } }).matchAll).toBe(false)
  })

  it('reports the roots a filter reads', () => {
    const f = compileFilter({ $or: [{ 'output.a': { $eq: 1 } }, { 'caller.x': { $any: { '@.y': { $eq: 1 } } } }] })
    expect([...f.roots].sort()).toEqual(['caller', 'output'])
  })
})

describe('normal forms (sdk/fixtures/filters/normalize)', () => {
  // Every document of the valid vectors and of the differential corpus.
  const docs: Doc[] = [
    ...jsonFiles(FIXTURES).flatMap((f) => readJson<ValidVector>(join(FIXTURES, f)).cases.map((c) => c.doc)),
    ...readJson<Corpus>(join(FIXTURES, 'diff', 'corpus.json')).runs.flatMap((r) => r.cases.map((c) => c.doc)),
  ]
  const files = jsonFiles(join(FIXTURES, 'normalize'))

  it.each(
    files,
  )('%s: the normal form compiles and agrees with its source on every vector and corpus document', (file) => {
    const vector = readJson<{ cases: { filters: unknown; normalized: unknown }[] }>(join(FIXTURES, 'normalize', file))
    for (const [i, c] of vector.cases.entries()) {
      const source = compileFilter(c.filters)
      const normal = compileFilter(c.normalized, { trusted: true })
      const disagree = docs.filter((d) => source.matches(asDoc(d)) !== normal.matches(asDoc(d)))
      expect(disagree, `case ${i}`).toEqual([])
    }
  })
})

describe('totality: reading the document never throws out of matches()', () => {
  const exists = compileFilter({ 'input.a': { $exists: true } })
  const ne = compileFilter({ 'input.a': { $ne: 1 } })
  const deep = compileFilter({ input: { $eq: { a: 1 } } })
  const hostile: [string, unknown][] = [
    [
      'a throwing getter',
      {
        get a() {
          throw new Error('getter')
        },
      },
    ],
    [
      'a throwing toJSON',
      {
        toJSON() {
          throw new Error('toJSON')
        },
      },
    ],
    [
      'a Proxy whose ownKeys trap throws',
      new Proxy(
        { a: 1 },
        {
          ownKeys() {
            throw new Error('ownKeys')
          },
        },
      ),
    ],
    [
      'a Proxy whose get trap throws',
      new Proxy(
        { a: 1 },
        {
          get() {
            throw new Error('get')
          },
        },
      ),
    ],
    [
      'a Proxy whose getOwnPropertyDescriptor trap throws',
      new Proxy(
        { a: 1 },
        {
          getOwnPropertyDescriptor() {
            throw new Error('getOwnPropertyDescriptor')
          },
        },
      ),
    ],
    [
      'a revoked Proxy',
      (() => {
        const { proxy, revoke } = Proxy.revocable({ a: 1 }, {})
        revoke()
        return proxy
      })(),
    ],
  ]

  it.each(hostile)('%s: no match, no throw', (name, input) => {
    // Only a comparison of the whole object lists its keys (the ownKeys trap).
    for (const f of name.includes('ownKeys') ? [deep] : [exists, ne, deep]) {
      expect(() => f.matches({ input })).not.toThrow()
      expect(f.matches({ input })).toBe(false)
    }
  })

  it('a literal nested thousands of levels deep is refused without overflowing the stack; 32 levels compare fine', () => {
    const nest = (depth: number, inner: unknown) => {
      let v = inner
      for (let i = 0; i < depth; i++) v = [v]
      return v
    }
    expect(errorCode(() => compileFilter({ input: { $eq: nest(4000, 1) } }))).toBe('INVALID_FILTER')
    const f = compileFilter({ input: { $eq: nest(32, 1) } })
    expect(f.matches({ input: nest(32, 1) })).toBe(true)
    expect(f.matches({ input: nest(32, 2) })).toBe(false)
    expect(f.matches({ input: nest(5000, 1) })).toBe(false) // a deep document is fine
  })
})

describe('a filter must be JSON as written', () => {
  class Clause {
    constructor() {
      Object.assign(this, { 'input.a': { $eq: 1 } })
    }
  }
  const cases: [string, unknown, string][] = [
    ['undefined operand', { 'input.a': { $eq: undefined } }, 'input.a.$eq'],
    ['undefined clause', { 'input.a': undefined }, 'input.a'],
    ['undefined in an array', { 'input.a': { $in: [1, undefined] } }, 'input.a.$in[1]'],
    // biome-ignore lint/suspicious/noSparseArray: a hole is what is tested
    ['a hole in an array', { 'input.a': { $in: [1, , 2] } }, 'input.a.$in'],
    ['NaN', { 'input.a': { $eq: Number.NaN } }, 'input.a.$eq'],
    ['Infinity', { 'input.a': { $lt: Number.POSITIVE_INFINITY } }, 'input.a.$lt'],
    ['-Infinity', { 'input.a': { $eq: Number.NEGATIVE_INFINITY } }, 'input.a.$eq'],
    ['a BigInt', { 'input.a': { $eq: 1n } }, 'input.a.$eq'],
    ['a function', { 'input.a': { $eq: () => 1 } }, 'input.a.$eq'],
    ['a symbol', { 'input.a': { $eq: Symbol('s') } }, 'input.a.$eq'],
    ['a Map as the filter', new Map([['input.a', { $eq: 1 }]]), 'the filter'],
    ['a class instance', new Clause(), 'the filter'],
    ['a Date operand', { 'input.at': { $eq: new Date(0) } }, 'input.at.$eq'],
    [
      'an accessor clause',
      Object.defineProperty({}, 'input.a', { get: () => ({ $eq: 1 }), enumerable: true }),
      'input.a',
    ],
    ['a non-enumerable clause', Object.defineProperty({}, 'input.a', { value: { $eq: 1 } }), 'input.a'],
    ['a symbol key', { [Symbol('k')]: { $eq: 1 } }, 'the filter'],
    ['a lone surrogate in a string', { 'input.s': { $eq: 'a\ud800b' } }, 'input.s.$eq'],
    ['a lone surrogate in a key', { 'input.\udc00': { $exists: true } }, 'input.\udc00'],
    [
      'a cycle',
      (() => {
        const c: Record<string, unknown> = {}
        c['input.a'] = { $eq: c }
        return c
      })(),
      '`input.a.$eq`: a cycle',
    ],
    [
      'a revoked Proxy',
      (() => {
        const { proxy, revoke } = Proxy.revocable({}, {})
        revoke()
        return proxy
      })(),
      'cannot be read',
    ],
  ]

  it.each(cases)('%s is INVALID_FILTER, naming where', (_name, filter, where) => {
    let error: unknown
    try {
      compileFilter(filter)
    } catch (e) {
      error = e
    }
    expect(error).toBeInstanceOf(FilterError)
    expect((error as FilterError).code).toBe('INVALID_FILTER')
    expect((error as FilterError).message).toContain(where)
  })

  it('a filter nested thousands of levels deep is refused as too deep, not with a stack overflow', () => {
    let deep: Record<string, unknown> = { 'input.a': { $eq: 1 } }
    for (let i = 0; i < 8000; i++) deep = { $and: [deep] }
    let error: unknown
    try {
      compileFilter(deep)
    } catch (e) {
      error = e
    }
    expect(error).toBeInstanceOf(FilterError)
    expect((error as FilterError).message).toContain('nests deeper than 120')
  })

  it('a value shared many times over (a DAG bomb) is refused quickly', () => {
    let bomb: Record<string, unknown> = { $eq: 1 }
    for (let i = 0; i < 40; i++) bomb = { a: bomb, b: bomb } // 2^40 paths, 41 objects
    const started = Date.now()
    expect(errorCode(() => compileFilter({ 'input.a': { $eq: bomb } }))).toBe('INVALID_FILTER')
    expect(Date.now() - started).toBeLessThan(1000)
  })

  it('accepts a value shared by two members (no cycle)', () => {
    const shared = { $in: [1, 2] }
    expect(compileFilter({ 'input.a': shared, 'input.b': shared }).matches({ input: { a: 1, b: 2 } })).toBe(true)
  })

  it('accepts prototype-less objects, and plain objects from another realm', () => {
    const bare = Object.assign(Object.create(null), { 'input.a': { $eq: 1 } })
    expect(compileFilter(bare).matches({ input: { a: 1 } })).toBe(true)
    const foreign = runInNewContext('({ "input.a": { "$eq": 1 } })')
    expect(compileFilter(foreign).matches({ input: { a: 1 } })).toBe(true)
  })

  it('freezes the JSON it compiled: source cannot be edited under the closures', () => {
    const src = { 'input.a': { $in: [1, 2] } }
    const f = compileFilter(src)
    expect(Object.isFrozen(f.source)).toBe(true)
    expect(Object.isFrozen((f.source['input.a'] as { $in: unknown[] }).$in)).toBe(true)
    src['input.a'].$in.push(3) // the caller's object stays its own
    expect(f.matches({ input: { a: 3 } })).toBe(false)
  })

  it('applies the 8 KiB size limit to filters as written, not to trusted (engine-validated) ones', () => {
    const big = { 'input.a': { $eq: 'x'.repeat(9000) } }
    expect(errorCode(() => compileFilter(big))).toBe('INVALID_FILTER')
    expect(compileFilter(big, { trusted: true }).matches({ input: { a: 'x'.repeat(9000) } })).toBe(true)
  })
})

describe('host values, continued', () => {
  it('a root inherited from the document prototype does not exist', () => {
    const doc = Object.create({ input: { a: 1 } })
    expect(compileFilter({ 'input.a': { $eq: 1 } }).matches(doc)).toBe(false)
    expect(compileFilter({ input: { $exists: false } }).matches(doc)).toBe(true)
  })

  it('a BigInt beyond the double range reads as null', () => {
    const doc = { input: { n: 2n ** 1100n } }
    expect(compileFilter({ 'input.n': { $eq: null } }).matches(doc)).toBe(true)
    expect(compileFilter({ 'input.n': { $exists: true } }).matches(doc)).toBe(true)
    expect(compileFilter({ 'input.n': { $gt: 0 } }).matches(doc)).toBe(false)
  })

  it('unboxes primitives from another realm', () => {
    const boxed = runInNewContext('({ n: new Number(5), s: new String("ab"), b: new Boolean(false) })')
    expect(compileFilter({ 'input.n': { $eq: 5 } }).matches({ input: boxed })).toBe(true)
    expect(compileFilter({ 'input.s': { $prefix: 'a' } }).matches({ input: boxed })).toBe(true)
    expect(compileFilter({ 'input.b': { $eq: false } }).matches({ input: boxed })).toBe(true)
  })
})
