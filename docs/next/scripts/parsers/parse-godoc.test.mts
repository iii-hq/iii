/**
 * Tests for parse-godoc.mts. Run from the repo root:
 *   pnpm tsx --test docs/next/scripts/parsers/parse-godoc.test.mts
 */
import assert from 'node:assert/strict'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { slugify } from '../renderers/components.mjs'
import { renderSdkMdx } from '../renderers/render-mdx.mjs'
import { parseGodoc, parseHelpersGodoc } from './parse-godoc.mjs'

const text = (t: string) => ({ kind: 'paragraph', spans: [{ kind: 'text', text: t }] })
const code = (c: string) => ({ kind: 'code', code: c })
const doc = (blocks: object[]) => ({ raw: '', blocks })

function pkg(relPath: string, extra: object = {}) {
  const name = relPath.split('/').pop()
  return {
    importPath: `example.com/m/${relPath}`,
    relPath,
    name,
    synopsis: `Package ${name} is a fixture.`,
    doc: doc([]),
    consts: [],
    vars: [],
    funcs: [],
    types: [],
    examples: [],
    ...extra,
  }
}

function writeModule(packages: object[]): string {
  const file = join(mkdtempSync(join(tmpdir(), 'parse-godoc-')), 'api-docs.json')
  writeFileSync(file, JSON.stringify({ module: 'example.com/m', goVersion: '1.25.0', packages }))
  return file
}

function parse(packages: object[]) {
  return parseHelpersGodoc(writeModule(packages))
}

const deprecatedDoc = (summary: string, note: string) => ({
  raw: `${summary}\n\nDeprecated: ${note}\n`,
  blocks: [text(summary)],
  deprecated: text(note),
})

function func(name: string, params: [string, string][], results: string[], d: object = doc([text(`${name} does it.`)])) {
  const list = params.map(([n, t]) => `${n} ${t}`).join(', ')
  const res = results.length > 1 ? ` (${results.join(', ')})` : results.length ? ` ${results[0]}` : ''
  return {
    name,
    signature: `func ${name}(${list})${res}`,
    funcType: `func(${list})${res}`,
    params: params.map(([n, t]) => ({ name: n, type: t })),
    results: results.map(t => ({ name: '', type: t })),
    doc: d,
    examples: [],
  }
}

function type(name: string, extra: object = {}) {
  return {
    name,
    kind: 'struct',
    doc: doc([text(`${name} is a type.`)]),
    fields: [],
    interfaceMethods: [],
    consts: [],
    vars: [],
    funcs: [],
    methods: [],
    examples: [],
    ...extra,
  }
}

// A package with a client, a handle, a method-like generic registration and
// deprecated types: one returned by a client method, one returned only by that
// deprecated handle (kept through the fixed point) and one only taken as a
// parameter.
const handlesTypes = () => [
  type('Client', {
    methods: [func('Open', [], ['*OldRef']), func('Use', [['in', 'OldInput']], ['error']), func('Do', [], [])],
  }),
  type('Gone', { doc: deprecatedDoc('Gone is gone.', 'nothing replaces it.') }),
  type('OldInput', { doc: deprecatedDoc('OldInput is an input.', 'use Ref instead.') }),
  type('OldRef', {
    doc: deprecatedDoc('OldRef is an old handle.', 'use Ref instead.'),
    methods: [func('Next', [], ['*Gone'])],
  }),
  type('Ref', {
    fields: [{ name: 'ID', type: 'string', doc: doc([text('ID is the id.')]) }],
    funcs: [func('NewRef', [], ['*Ref']), func('RegisterTyped', [['c', '*Client'], ['id', 'string']], ['*Ref', 'error'])],
    methods: [func('Unregister', [], ['error'])],
  }),
]

test('the import line uses the name the package is imported under', () => {
  const doc = parse([pkg('helpers/http', { importName: 'myhttp' }), pkg('helpers/queue')])
  assert.deepEqual(
    doc.modules?.map(m => m.importPath),
    ['import myhttp "example.com/m/helpers/http"', 'import "example.com/m/helpers/queue"'],
  )
})

test('a lead-in ending with a colon moves with its code into the examples', () => {
  const withX = {
    name: 'WithX',
    signature: 'func WithX() Option',
    funcType: 'func() Option',
    params: [],
    results: [{ name: '', type: 'Option' }],
    doc: doc([
      text('WithX sets X.'),
      code('c := New(WithX())'),
      text('To turn X off:'),
      code('c := New()'),
      text('It covers two cases:'),
      { kind: 'list', items: [[text('one')], [text('two')]] },
    ]),
    examples: [],
  }
  const fn = parse([pkg('helpers/x', { funcs: [withX] })]).modules?.[0].functions[0]
  assert.equal(fn?.description, 'WithX sets X.\n\nIt covers two cases:\n\n- one\n- two')
  assert.deepEqual(fn?.examples, ['c := New(WithX())', '// To turn X off:\nc := New()'])
})

test('a long lead-in wraps into comment lines of at most 80 columns', () => {
  const lead = `${'word '.repeat(30).trim()}:`
  const f = {
    name: 'F',
    signature: 'func F()',
    funcType: 'func()',
    params: [],
    results: [],
    doc: doc([text(lead), code('F()')]),
    examples: [],
  }
  const example = parse([pkg('helpers/f', { funcs: [f] })]).modules?.[0].functions[0].examples[0] ?? ''
  const lines = example.split('\n')
  assert.equal(lines.at(-1), 'F()')
  for (const line of lines.slice(0, -1)) {
    assert.ok(line.startsWith('// ') && line.length <= 80, line)
  }
  assert.equal(lines.slice(0, -1).map(l => l.slice(3)).join(' '), lead)
})

test('a method-like function is no constructor row of the handle it returns', () => {
  const mod = parse([pkg('helpers/h', { types: handlesTypes() })]).modules?.[0]
  const ref = mod?.types.find(t => t.name === 'Ref')
  assert.deepEqual(ref?.fields.map(f => f.name), ['ID', 'NewRef', 'Unregister'])
  // Both stay under Functions.
  assert.deepEqual(mod?.functions.map(f => f.name), ['NewRef', 'RegisterTyped'])
})

test('deprecated types the API returns stay in Types, marked deprecated', () => {
  const mod = parse([pkg('helpers/h', { types: handlesTypes() })]).modules?.[0]
  assert.deepEqual(mod?.types.map(t => t.name).sort(), ['Client', 'Gone', 'OldRef', 'Ref'])
  const oldRef = mod?.types.find(t => t.name === 'OldRef')
  assert.equal(oldRef?.description, '**Deprecated.** Use Ref instead.\n\nOldRef is an old handle.')
  assert.ok(oldRef?.fields.some(f => f.name === 'Next'))
})

test('on the SDK page a returned deprecated handle is a type, a deprecated input only expands', () => {
  const sdk = parseGodoc(writeModule([pkg('', { importPath: 'example.com/m', name: 'm', types: handlesTypes() })]))
  assert.deepEqual(sdk.types.map(t => t.name), ['Gone', 'OldRef', 'Ref'])
  assert.deepEqual(sdk.paramOnlyTypes?.map(t => t.name), ['OldInput'])
  assert.deepEqual(sdk.methods.map(m => m.name), ['Client.Open', 'Client.Use', 'Client.Do'])
})

test('Client method headings carry the receiver and keep the canonical order', () => {
  const types = [
    type('Client', {
      methods: [func('Address', [], ['string']), func('Call', [], ['error']), func('RegisterFunction', [], ['*Call', 'error'])],
    }),
    type('Call', { methods: [func('Matches', [], ['bool'])] }),
  ]
  const mdx = renderSdkMdx(parseGodoc(writeModule([pkg('', { importPath: 'example.com/m', name: 'm', types })])))
  const headings = [...mdx.matchAll(/^#{3,4} (.+)$/gm)].map(m => m[1])
  const methods = headings.slice(headings.indexOf('Client.RegisterFunction'), headings.indexOf('Client.Address') + 1)
  assert.deepEqual(methods, ['Client.RegisterFunction', 'Client.Call', 'Client.Address'])
  // The type keeps the bare name, so its anchor (#call) is its own.
  const slug = (h: string) => h.toLowerCase().replace(/[^\w]+/g, '-').replace(/^-+|-+$/g, '')
  assert.equal(headings.filter(h => slug(h) === 'call').length, 1)
  assert.match(mdx, /\[`Call`\]\(#call\)/)
})

test('anchors use the slug Mintlify gives a heading, and every link has exactly one target', () => {
  // Ids observed on the Mintlify preview: the dot of a qualified heading becomes a hyphen.
  assert.equal(slugify('Client.OnObserverDeliverFor'), 'client-onobserverdeliverfor')
  assert.equal(slugify('filter.Compile'), 'filter-compile')
  assert.equal(slugify('iii/filter'), 'iii-filter')
  const types = [
    type('Client', {
      methods: [func('Call', [], ['*Call', 'error']), func('ConnectionState', [], ['ConnectionState'])],
    }),
    type('Call', { methods: [func('Matches', [], ['bool'])] }),
    type('ConnectionState', { kind: 'named', decl: 'type ConnectionState string' }),
  ]
  const mdx = renderSdkMdx(parseGodoc(writeModule([pkg('', { importPath: 'example.com/m', name: 'm', types })])))
  const prose = mdx.replace(/```[\s\S]*?```/g, '')
  const slugs = [...prose.matchAll(/^#{1,6} (.+)$/gm)].map(m => slugify(m[1]))
  const links = [...new Set([...prose.matchAll(/\]\(#([^)]+)\)/g)].map(m => m[1]))]
  assert.ok(links.includes('call') && links.includes('connectionstate'), links.join(' '))
  for (const link of links) {
    assert.equal(slugs.filter(s => s === link).length, 1, `#${link}`)
  }
  assert.ok(slugs.includes('client-call') && slugs.includes('client-connectionstate'))
})
