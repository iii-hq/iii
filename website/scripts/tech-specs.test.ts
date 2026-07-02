import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { readTechSpecs } from './tech-specs'

async function makeSpecTree(specs: Record<string, string | null>): Promise<string> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'tech-specs-'))
  for (const [slug, readme] of Object.entries(specs)) {
    await fs.mkdir(path.join(dir, slug), { recursive: true })
    if (readme !== null) await fs.writeFile(path.join(dir, slug, 'README.md'), readme, 'utf8')
  }
  return dir
}

test('reads frontmatter title, date, and status', async () => {
  const dir = await makeSpecTree({
    '2026-06-devexp': '---\ntitle: the devexp overhaul\ndate: 2026-06\nstatus: live\n---\n\n# ignored h1\n',
  })
  const specs = await readTechSpecs(dir)
  assert.deepEqual(specs, [{ slug: '2026-06-devexp', title: 'the devexp overhaul', date: '2026-06', status: 'live' }])
})

test('falls back to H1 title and dirname date when frontmatter is absent', async () => {
  const dir = await makeSpecTree({
    '2026-07-storage': '# Storage Worker\n\nprose.\n',
  })
  const specs = await readTechSpecs(dir)
  assert.deepEqual(specs, [{ slug: '2026-07-storage', title: 'Storage Worker', date: '2026-07', status: 'live' }])
})

test('day-precision dates: frontmatter YYYY-MM-DD and the YYYY-MM-DD dirname prefix', async () => {
  const dir = await makeSpecTree({
    '2026-06-29-codegen': '---\ntitle: codegen\ndate: 2026-06-29\n---\n# codegen\n',
    '2026-06-08-agentic': '# agentic\n\nprose.\n',
  })
  const specs = await readTechSpecs(dir)
  assert.deepEqual(specs, [
    { slug: '2026-06-29-codegen', title: 'codegen', date: '2026-06-29', status: 'live' },
    { slug: '2026-06-08-agentic', title: 'agentic', date: '2026-06-08', status: 'live' },
  ])
})

test('skips specs without a README and dirs without a derivable date', async () => {
  const dir = await makeSpecTree({
    '2026-06-a': null,
    'no-date-prefix': '# something\n',
    '2026-06-b': '---\ntitle: b\n---\n# b\n',
  })
  const specs = await readTechSpecs(dir)
  assert.deepEqual(
    specs.map((s) => s.slug),
    ['2026-06-b'],
  )
})

test('marks drafts and sorts newest first', async () => {
  const dir = await makeSpecTree({
    '2026-05-old': '---\ntitle: old\n---\n# old\n',
    '2026-07-draft': '---\ntitle: wip\nstatus: draft\n---\n# wip\n',
  })
  const specs = await readTechSpecs(dir)
  assert.deepEqual(
    specs.map((s) => [s.slug, s.status]),
    [
      ['2026-07-draft', 'draft'],
      ['2026-05-old', 'live'],
    ],
  )
})

test('missing tech-specs dir yields an empty list', async () => {
  assert.deepEqual(await readTechSpecs('/nonexistent/tech-specs'), [])
})
