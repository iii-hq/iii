import { readdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { type CompiledFilter, compileFilter, type FilterDoc, FilterIndex } from '../src/filter'

/**
 * The index of the filters' equality conditions (decision 55) against the
 * evaluator: for seeded random filters and documents, `FilterIndex.matches`
 * returns exactly the entries that evaluating every filter returns, in order,
 * through inserts, removals and replacements. Corpora: a generator over every
 * operator; an equality-heavy one, where many filters share paths and values
 * and documents respell them (types, `-0`, `null` and absent paths, arrays,
 * nested paths, host values); every conformance vector in one index; the
 * Rust differential corpus.
 */
const FIXTURES = resolve(__dirname, '../../../../fixtures/filters')

type Json = Record<string, unknown>

function readJson<T>(path: string): T {
  return JSON.parse(readFileSync(path, 'utf8')) as T
}

/** mulberry32: deterministic for a seed. */
class Rng {
  private state: number

  constructor(seed: number) {
    this.state = seed >>> 0
  }

  next(): number {
    this.state = (this.state + 0x6d2b79f5) >>> 0
    let t = this.state
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }

  below(n: number): number {
    return Math.floor(this.next() * n)
  }

  chance(percent: number): boolean {
    return this.next() * 100 < percent
  }

  pick<T>(items: readonly T[]): T {
    return items[this.below(items.length)]
  }
}

type Entry = { key: number; filter: CompiledFilter; source: unknown }

/** What evaluating every filter, in key order, matches. */
function scan(entries: readonly Entry[], doc: FilterDoc, accept?: (key: number) => boolean): number[] {
  return [...entries]
    .sort((a, b) => a.key - b.key)
    .filter((e) => (accept === undefined || accept(e.key)) && e.filter.matches(doc))
    .map((e) => e.key)
}

function describeEntries(entries: readonly Entry[], keys: Set<number>): string {
  return entries
    .filter((e) => keys.has(e.key))
    .map((e) => `  ${e.key}: ${JSON.stringify(e.source)}`)
    .join('\n')
}

/** Asserts that the index and a scan agree on `doc`; returns how many filters the index evaluated. */
function agree(
  index: FilterIndex<number>,
  entries: readonly Entry[],
  doc: FilterDoc,
  context: string,
  accept?: (key: number) => boolean,
): number {
  const expected = scan(entries, doc, accept)
  const got = index.matches(doc, accept)
  if (JSON.stringify(got) !== JSON.stringify(expected)) {
    const differ = new Set([...expected.filter((k) => !got.includes(k)), ...got.filter((k) => !expected.includes(k))])
    throw new Error(
      `${context}: the index matched ${JSON.stringify(got)}, a scan ${JSON.stringify(expected)}\n` +
        `doc ${safeJson(doc)}\nfilters that differ:\n${describeEntries(entries, differ)}`,
    )
  }
  return index.candidates(doc)
}

function safeJson(value: unknown): string {
  try {
    return JSON.stringify(value)
  } catch {
    return String(value)
  }
}

/** A generated filter the language accepts (a few exceed the node limit and are generated again). */
function valid(gen: () => Json): Json {
  for (;;) {
    const source = gen()
    try {
      compileFilter(source)
      return source
    } catch {
      // over a limit: another one
    }
  }
}

function build(sources: readonly unknown[]): { index: FilterIndex<number>; entries: Entry[] } {
  const index = new FilterIndex<number>()
  const entries: Entry[] = []
  sources.forEach((source, key) => {
    const filter = compileFilter(source)
    index.insert(key, filter, key)
    entries.push({ key, filter, source })
  })
  return { index, entries }
}

// ── the generator over every operator ─────────────────────────────────────

const ROOTS = ['input', 'input', 'input', 'output', 'output', 'error', 'caller', 'metadata']
const KEYS = [
  'a',
  'b',
  'id',
  'n',
  's',
  'tags',
  'items',
  'status',
  '0',
  '1',
  '01',
  'x\\.y',
  'é',
  'length',
  'constructor',
]
const SCALARS: readonly unknown[] = [
  0,
  -0,
  1,
  1.5,
  -1,
  2 ** 53,
  -(2 ** 53),
  0.1,
  '',
  '0',
  '1',
  'a',
  'b',
  'ab',
  'é',
  '😀',
  true,
  false,
  null,
]
const OPERATORS = [
  '$eq',
  '$eq',
  '$eq',
  '$ne',
  '$in',
  '$in',
  '$nin',
  '$gt',
  '$gte',
  '$lt',
  '$lte',
  '$exists',
  '$prefix',
  '$any',
]

function genPath(rng: Rng): string {
  const segments = Array.from({ length: rng.below(3) }, () => rng.pick(KEYS))
  return [rng.pick(ROOTS), ...segments].join('.')
}

function genLiteral(rng: Rng, depth = 0): unknown {
  const roll = rng.below(100)
  if (depth < 2 && roll < 10) return Array.from({ length: rng.below(3) }, () => genLiteral(rng, depth + 1))
  if (depth < 2 && roll < 18) return { [rng.pick(['a', 'k', 'n'])]: genLiteral(rng, depth + 1) }
  return rng.pick(SCALARS)
}

function genOperand(rng: Rng, op: string, depth: number): unknown {
  switch (op) {
    case '$eq':
    case '$ne':
      return genLiteral(rng)
    case '$in':
    case '$nin':
      return Array.from({ length: rng.below(4) }, () => (rng.chance(85) ? rng.pick(SCALARS) : genLiteral(rng)))
    case '$gt':
    case '$gte':
    case '$lt':
    case '$lte':
      return rng.chance(60) ? rng.pick([0, 1, -1, 1.5, 2 ** 53]) : rng.pick(['', 'a', 'b', 'é'])
    case '$exists':
      return rng.chance(50)
    case '$prefix':
      return rng.pick(['', 'a', 'é', '😀'])
    default: {
      // $any: an operator object, or a filter over `@`
      if (rng.chance(50) || depth >= 3) return { [rng.pick(['$eq', '$gte', '$in'])]: rng.pick([1, 'a', null]) }
      return { [`@${rng.chance(50) ? '' : `.${rng.pick(['a', 'n'])}`}`]: { $eq: rng.pick(SCALARS) } }
    }
  }
}

function genFilter(rng: Rng, depth = 1): Json {
  const filter: Json = {}
  const clauses = 1 + rng.below(3)
  for (let i = 0; i < clauses; i++) {
    const roll = rng.below(100)
    if (depth < 4 && roll < 8) {
      filter.$or = Array.from({ length: 1 + rng.below(2) }, () => genFilter(rng, depth + 1))
    } else if (depth < 4 && roll < 14) {
      filter.$not = genFilter(rng, depth + 1)
    } else if (depth < 4 && roll < 22) {
      filter.$and = Array.from({ length: 1 + rng.below(2) }, () => genFilter(rng, depth))
    } else {
      const ops: Json = {}
      for (let j = 0; j < 1 + rng.below(2); j++) {
        const op = rng.pick(OPERATORS)
        ops[op] = genOperand(rng, op, depth)
      }
      filter[genPath(rng)] = ops
    }
  }
  return filter
}

/**
 * Sets `path` (dotted, `\.` escaped) in `doc` to `value`, making objects on
 * the way, or an array before an index segment when `rng` says so.
 */
function setPath(rng: Rng, doc: Json, path: string, value: unknown): void {
  const parts = path.split(/(?<!\\)\./).map((p) => p.replace(/\\\./g, '.'))
  const index = (key: string) => /^(0|[1-9][0-9]*)$/.test(key) && key.length < 4
  let current: Json | unknown[] = doc
  for (let i = 0; i < parts.length; i++) {
    const key = parts[i]
    // an array made earlier holds elements only: the path stays unset
    if (Array.isArray(current) && !index(key)) return
    const holder = current as Record<string, unknown>
    if (i === parts.length - 1) {
      holder[key] = value
      return
    }
    let next = holder[key]
    if (next === null || typeof next !== 'object') {
      next = index(parts[i + 1]) && rng.chance(50) ? [] : {}
      holder[key] = next
    }
    current = next as Json
  }
}

/** Every `(path, operand)` a filter names, at any depth, `@` paths left out. */
function namedPaths(filter: unknown, out: [string, unknown][] = []): [string, unknown][] {
  if (filter === null || typeof filter !== 'object' || Array.isArray(filter)) return out
  for (const [key, value] of Object.entries(filter as Json)) {
    if (key === '$and' || key === '$or') for (const item of value as unknown[]) namedPaths(item, out)
    else if (key === '$not') namedPaths(value, out)
    else if (!key.startsWith('@')) {
      for (const operand of Object.values(value as Json)) out.push([key, operand])
    }
  }
  return out
}

/** A document shaped by `filters`: their paths usually present, with nearby values, noise and other types. */
function genDoc(rng: Rng, filters: readonly unknown[]): Json {
  const doc: Json = {}
  for (const filter of filters) {
    for (const [path, operand] of namedPaths(filter)) {
      if (!rng.chance(55)) continue
      let value: unknown
      const roll = rng.below(100)
      if (roll < 45 && operand !== null && typeof operand !== 'object') value = operand
      else if (roll < 55 && Array.isArray(operand) && operand.length > 0) value = rng.pick(operand)
      else if (roll < 85) value = rng.pick(SCALARS)
      else value = genLiteral(rng)
      setPath(rng, doc, path, value)
    }
  }
  return doc
}

function asDoc(doc: Json): FilterDoc {
  return { input: doc.input, output: doc.output, error: doc.error, caller: doc.caller, metadata: doc.metadata }
}

describe('FilterIndex: the generator over every operator', () => {
  it.each([1, 2, 3, 4, 5, 6, 7, 8])('matches what a scan matches (seed %i)', (seed) => {
    const rng = new Rng(seed)
    const sources = Array.from({ length: 300 }, () => valid(() => genFilter(rng)))
    const { index, entries } = build(sources)
    for (let i = 0; i < 300; i++) {
      const doc = asDoc(genDoc(rng, [rng.pick(sources), rng.pick(sources), rng.pick(sources)]))
      agree(index, entries, doc, `seed ${seed}, doc ${i}`)
    }
  })
})

// ── the equality-heavy corpus ─────────────────────────────────────────────

const SHARED_PATHS = [
  'input.k',
  'input.k',
  'input.tenant',
  'input.a.b',
  'input.items.0',
  'input.x\\.y',
  'input',
  'output.status',
  'error.code',
  'caller.worker_name',
  'metadata.tenant',
]
const SHARED_VALUES: readonly unknown[] = [0, -0, 1, 2, 1.5, '1', 'a', 'b', true, false, null]

function sharedScalar(rng: Rng): unknown {
  return rng.pick(SHARED_VALUES)
}

function sharedLiteral(rng: Rng): unknown {
  const roll = rng.below(100)
  if (roll < 6) return [sharedScalar(rng)]
  if (roll < 10) return { k: sharedScalar(rng) }
  return sharedScalar(rng)
}

/** One to three clauses on shared paths, mostly `$eq` / `$in`, with every other kind of clause mixed in. */
function equalityFilter(rng: Rng): Json {
  const clauses: Json[] = []
  for (let i = 0; i < 1 + rng.below(3); i++) {
    const path = rng.pick(SHARED_PATHS)
    const roll = rng.below(100)
    let ops: Json
    if (roll < 45) ops = { $eq: sharedLiteral(rng) }
    else if (roll < 62) ops = { $in: Array.from({ length: rng.below(4) }, () => sharedLiteral(rng)) }
    else if (roll < 68) ops = { $eq: sharedLiteral(rng), $ne: sharedLiteral(rng) }
    else if (roll < 72) ops = { $in: [], $exists: true }
    else if (roll < 78) ops = { $gte: rng.pick([0, 1, 'a']) }
    else if (roll < 82) ops = { $nin: [sharedLiteral(rng)] }
    else if (roll < 86) ops = { $exists: rng.chance(50) }
    else if (roll < 90) ops = { $any: { $eq: sharedScalar(rng) } }
    else if (roll < 95) {
      clauses.push({ $or: [{ [path]: { $eq: sharedScalar(rng) } }, { [rng.pick(SHARED_PATHS)]: { $exists: true } }] })
      continue
    } else {
      clauses.push({ $not: { [path]: { $eq: sharedScalar(rng) } } })
      continue
    }
    clauses.push({ [path]: ops })
  }
  // the same conditions written flat, under `$and`, or in a nested `$and`
  const roll = rng.below(3)
  if (roll === 0) return Object.assign({}, ...clauses.filter((c) => !('$or' in c) && !('$not' in c)), { $and: clauses })
  if (roll === 1) return { $and: clauses }
  return { $and: [{ $and: clauses }] }
}

/** A host value as a call hands it over: respelled, boxed, a BigInt, a Date, NaN, a getter that throws. */
function hostValue(rng: Rng): unknown {
  const roll = rng.below(100)
  if (roll < 55) return sharedScalar(rng)
  if (roll < 60) return [sharedScalar(rng), sharedScalar(rng)]
  if (roll < 64) return { k: sharedScalar(rng) }
  if (roll < 66) return []
  if (roll < 69) return BigInt(rng.pick([0, 1, 2]))
  if (roll < 72) return new Number(rng.pick([0, 1, 1.5]))
  if (roll < 74) return new String(rng.pick(['a', '1']))
  if (roll < 76) return new Boolean(rng.chance(50))
  if (roll < 78) return Number.NaN
  if (roll < 80) return Number.POSITIVE_INFINITY
  if (roll < 82) return undefined
  if (roll < 84) return () => 1
  if (roll < 86) {
    const json = sharedScalar(rng)
    return { toJSON: () => json }
  }
  if (roll < 88) return new Date(0)
  return '2'
}

/** A document setting some shared paths to shared values, respelled, or to other types; others absent. */
function equalityDoc(rng: Rng): Json {
  const doc: Json = {}
  for (const path of SHARED_PATHS) {
    if (!rng.chance(60)) continue
    if (path === 'input') {
      if (rng.chance(20)) doc.input = hostValue(rng)
      continue
    }
    setPath(rng, doc, path, hostValue(rng))
  }
  if (rng.chance(4)) {
    // a path whose reading throws
    const input = (doc.input !== null && typeof doc.input === 'object' ? doc.input : {}) as Json
    Object.defineProperty(input, 'k', {
      enumerable: true,
      get() {
        throw new Error('unreadable')
      },
    })
    doc.input = input
  }
  return doc
}

describe('FilterIndex: shared paths and values', () => {
  it.each([11, 12, 13, 14, 15, 16])('matches what a scan matches, and indexes most entries (seed %i)', (seed) => {
    const rng = new Rng(seed)
    const sources = Array.from({ length: 400 }, () => valid(() => equalityFilter(rng)))
    const { index, entries } = build(sources)
    let evaluated = 0
    for (let i = 0; i < 400; i++) {
      evaluated += agree(index, entries, asDoc(equalityDoc(rng)), `seed ${seed}, doc ${i}`)
    }
    // most filters hold an equality condition: a call evaluates a fraction of them
    expect(evaluated / 400).toBeLessThan(sources.length / 2)
  })

  it('with an accept check: asked before each candidate is evaluated, and what it refuses is left out', () => {
    const rng = new Rng(21)
    const sources = Array.from({ length: 200 }, () => valid(() => equalityFilter(rng)))
    const { index, entries } = build(sources)
    for (let i = 0; i < 200; i++) {
      agree(index, entries, asDoc(equalityDoc(rng)), `doc ${i}`, (key) => key % 3 !== 0)
    }
  })
})

describe('FilterIndex: inserts, removals and replacements', () => {
  it.each([31, 32, 33, 34])('stays exact through churn (seed %i)', (seed) => {
    const rng = new Rng(seed)
    const index = new FilterIndex<string>()
    // what the index holds: each value with its filter and order
    const model = new Map<string, { order: number; filter: CompiledFilter }>()
    let next = 0
    for (let step = 0; step < 1500; step++) {
      const roll = rng.below(100)
      const value = `v${rng.below(120)}`
      if (roll < 55) {
        // inserted, or replaced; a replacement may keep its order, so it is out of insertion order
        const filter = compileFilter(valid(() => (rng.chance(70) ? equalityFilter(rng) : genFilter(rng))))
        const held = model.get(value)
        const order = held && rng.chance(50) ? held.order : ++next
        index.insert(value, filter, order)
        model.set(value, { order, filter })
      } else if (roll < 80) {
        index.remove(value)
        model.delete(value)
      } else {
        const doc = asDoc(equalityDoc(rng))
        const expected = [...model]
          .sort((a, b) => a[1].order - b[1].order)
          .filter(([, held]) => held.filter.matches(doc))
          .map(([v]) => v)
        expect(index.matches(doc), `step ${step}`).toEqual(expected)
      }
      expect(index.size).toBe(model.size)
    }
  })

  it('a removed or replaced entry leaves no trace in its old bucket', () => {
    const index = new FilterIndex<string>()
    index.insert('a', compileFilter({ 'input.k': { $eq: 1 } }), 1)
    index.insert('b', compileFilter({ 'input.k': { $in: [1, 2] } }), 2)
    expect(index.matches({ input: { k: 1 } })).toEqual(['a', 'b'])
    index.insert('a', compileFilter({ 'input.k': { $eq: 2 } }), 1)
    expect(index.matches({ input: { k: 1 } })).toEqual(['b'])
    expect(index.matches({ input: { k: 2 } })).toEqual(['a', 'b'])
    index.remove('b')
    expect(index.matches({ input: { k: 2 } })).toEqual(['a'])
    expect(index.candidates({ input: { k: 1 } })).toBe(0)
    index.remove('a')
    expect(index.size).toBe(0)
    expect(index.candidates({ input: { k: 2 } })).toBe(0)
  })
})

describe('FilterIndex: what it evaluates', () => {
  it('1,000 filters on their own input.k: a call evaluates one', () => {
    const { index } = build(Array.from({ length: 1000 }, (_, k) => ({ 'input.k': { $eq: k } })))
    expect(index.matches({ input: { k: 42 } })).toEqual([42])
    expect(index.candidates({ input: { k: 42 } })).toBe(1)
    expect(index.matches({ input: { k: 1000 } })).toEqual([])
    expect(index.candidates({ input: { k: 1000 } })).toBe(0)
    expect(index.candidates({ input: {} })).toBe(0)
  })

  it('files a filter under an equality condition of its top-level $and only, and scans the rest', () => {
    const scanned = [
      {},
      { 'input.k': { $ne: 1 } },
      { 'input.k': { $gte: 1 } },
      { 'input.k': { $exists: true } },
      { 'input.k': { $prefix: 'a' } },
      { 'input.k': { $nin: [1] } },
      { 'input.k': { $in: [] } },
      { 'input.k': { $eq: [1] } },
      { 'input.k': { $eq: { a: 1 } } },
      { 'input.k': { $in: [1, [1]] } },
      { $or: [{ 'input.k': { $eq: 1 } }] },
      { $not: { 'input.k': { $eq: 1 } } },
      { 'input.list': { $any: { $eq: 1 } } },
      { 'input.list': { $any: { '@': { $eq: 1 } } } },
    ]
    const filed = [
      { 'input.k': { $eq: 1 } },
      { 'input.k': { $eq: 1, $ne: 2 } },
      { 'input.k': { $in: [1, 2] } },
      { $and: [{ 'input.k': { $eq: 1 } }] },
      { $and: [{ $and: [{ 'input.k': { $eq: 1 } }] }] },
      { 'input.k': { $eq: 1 }, $or: [{ 'input.n': { $gte: 0 } }] },
    ]
    const { index } = build([...scanned, ...filed])
    // a document whose input.k equals none of the filed values: only the scanned list is evaluated
    expect(index.candidates({ input: { k: 3 } })).toBe(scanned.length)
    expect(index.candidates({ input: { k: 1 } })).toBe(scanned.length + filed.length)
  })

  it('numbers by value: 1 and 1.0 are one value, -0 is 0; a value of one type never equals another', () => {
    const { index } = build([
      { 'input.k': { $eq: 0 } },
      { 'input.k': { $eq: -0 } },
      { 'input.k': { $eq: 1 } },
      { 'input.k': { $eq: '1' } },
      { 'input.k': { $eq: true } },
      { 'input.k': { $eq: null } },
    ])
    expect(index.matches({ input: { k: -0 } })).toEqual([0, 1])
    expect(index.matches({ input: { k: 1.0 } })).toEqual([2])
    expect(index.matches({ input: { k: 1n } })).toEqual([2])
    expect(index.matches({ input: { k: new Number(1) } })).toEqual([2])
    expect(index.matches({ input: { k: '1' } })).toEqual([3])
    expect(index.matches({ input: { k: true } })).toEqual([4])
    expect(index.matches({ input: { k: null } })).toEqual([5])
    // NaN and the infinities read as null, as JSON writes them
    expect(index.matches({ input: { k: Number.NaN } })).toEqual([5])
    // absent is not null
    expect(index.matches({ input: {} })).toEqual([])
    expect(index.matches({ input: { k: undefined } })).toEqual([])
  })

  it('a path whose reading throws matches none of the filters filed under it', () => {
    const { index, entries } = build([{ 'input.k': { $eq: 1 } }, { 'input.n': { $exists: false } }])
    const input = {}
    Object.defineProperty(input, 'k', {
      enumerable: true,
      get() {
        throw new Error('unreadable')
      },
    })
    agree(index, entries, { input }, 'a throwing getter')
    expect(index.matches({ input })).toEqual([1])
  })
})

// ── the shared vectors and the Rust corpus ────────────────────────────────

type ValidVector = { filters: unknown; cases: { doc: Json }[] }
type Corpus = { runs: { seed: number; cases: { filters: unknown; doc: Json; expect: boolean }[] }[] }

describe('FilterIndex: the conformance vectors and the differential corpus', () => {
  const vectors = readdirSync(FIXTURES)
    .filter((f) => f.endsWith('.json'))
    .sort()
    .map((f) => readJson<ValidVector>(join(FIXTURES, f)))
  const corpus = readJson<Corpus>(join(FIXTURES, 'diff', 'corpus.json'))
  const docs = [
    ...vectors.flatMap((v) => v.cases.map((c) => c.doc)),
    ...corpus.runs.flatMap((r) => r.cases.map((c) => c.doc)),
  ]

  it('every vector filter in one index agrees with a scan on every vector and corpus document', () => {
    expect(vectors.length).toBeGreaterThan(100)
    const { index, entries } = build(vectors.map((v) => v.filters))
    for (const [i, doc] of docs.entries()) agree(index, entries, asDoc(doc), `document ${i}`)
  })

  it.each(
    corpus.runs.map((r) => [r.seed, r] as const),
  )('the filters of corpus seed %i in one index agree with a scan, and with Rust', (_seed, run) => {
    const { index, entries } = build(run.cases.map((c) => c.filters))
    for (const [i, c] of run.cases.entries()) {
      const doc = asDoc(c.doc)
      agree(index, entries, doc, `case ${i}`)
      // the case's own filter is matched exactly when the Rust evaluator says it is
      expect(index.matches(doc).includes(i), `case ${i}`).toBe(c.expect)
    }
  })
})
