/**
 * Tests for parse-godoc.mts. Run from the repo root:
 *   pnpm tsx --test docs/next/scripts/parsers/parse-godoc.test.mts
 */
import assert from 'node:assert/strict'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { parseHelpersGodoc } from './parse-godoc.mjs'

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

function parse(packages: object[]) {
  const file = join(mkdtempSync(join(tmpdir(), 'parse-godoc-')), 'api-docs.json')
  writeFileSync(file, JSON.stringify({ module: 'example.com/m', goVersion: '1.25.0', packages }))
  return parseHelpersGodoc(file)
}

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
