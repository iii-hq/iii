#!/usr/bin/env node
/**
 * build.mjs — assembles the whole tech-specs site into dist/.
 *
 * Output layout (deployed to iii.dev under /roadmap/ by
 * .github/workflows/deploy-website.yml):
 *
 *   dist/index.html            the gallery (index of every spec)
 *   dist/index.json            machine-readable spec list (landing-page timeline)
 *   dist/<slug>/index.html     one page per spec: the interactive deck when
 *                              website/roadmap/<slug>/ exists, otherwise
 *                              the generic markdown spec viewer
 *   dist/<slug>/<file>.md      raw spec markdown, directly linkable
 *
 * Contracts enforced here (fail fast, named paths):
 *   - every deck dir pairs with a tech-specs/<slug>/ spec dir (no orphans)
 *   - deck index.html entries are relative (./src/main.tsx) so the single dev
 *     server and the /roadmap/ prefix both work
 *   - every shared src component has a COMPONENTS.md entry (warn; hard fail
 *     with --strict-registry)
 *
 * Usage:
 *   node build.mjs                     build everything
 *   node build.mjs --only=<slug>       gallery + one spec (fast rebuild)
 *   node build.mjs --strict-registry   registry parity warnings become errors
 */

import {
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { join } from 'node:path'
import { build } from 'vite'
import { listSpecDocs, readSpecs, ROOT, SPECS_DIR } from './scripts/manifest.mjs'

const DIST = join(ROOT, 'dist')
const VIEWER_TMP = join(DIST, '_viewer-tmp')
// top-level dirs of this project that are never a deck
const NOT_A_DECK = new Set(['src', 'scripts', 'dist', 'node_modules', '_viewer'])

const args = process.argv.slice(2)
const onlyEq = args.find((a) => a.startsWith('--only='))?.slice('--only='.length)
const onlyPos = args.includes('--only') ? args[args.indexOf('--only') + 1] : undefined
const ONLY = onlyEq ?? (onlyPos && !onlyPos.startsWith('--') ? onlyPos : undefined)
const STRICT_REGISTRY = args.includes('--strict-registry')

/** @param {string} root @param {string} outDir @param {string} label */
async function buildApp(root, outDir, label) {
  console.log(`\n▸ ${label}`)
  await build({
    configFile: join(ROOT, 'vite.config.ts'),
    root,
    logLevel: 'warn',
    build: { outDir, emptyOutDir: false },
  })
}

/** deck dirs present in this project (top-level, contain an index.html) */
function discoverDeckDirs() {
  return readdirSync(ROOT, { withFileTypes: true })
    .filter((e) => e.isDirectory() && !NOT_A_DECK.has(e.name) && !e.name.startsWith('.'))
    .map((e) => e.name)
    .filter((name) => existsSync(join(ROOT, name, 'index.html')))
    .sort()
}

/** every shared src file must have a `### <basename>` entry in COMPONENTS.md */
function checkRegistry() {
  const registryPath = join(ROOT, 'COMPONENTS.md')
  const registry = existsSync(registryPath) ? readFileSync(registryPath, 'utf8') : ''
  const entries = new Set([...registry.matchAll(/^### (\S+)$/gm)].map((m) => m[1]))
  const skip = new Set(['main.tsx', 'App.tsx', 'vite-env.d.ts', 'index.css'])
  /** @type {string[]} */
  const problems = []
  const files = []
  const walk = (/** @type {string} */ dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, entry.name)
      if (entry.isDirectory()) walk(p)
      else if (/\.(tsx?|mts)$/.test(entry.name) && !skip.has(entry.name)) files.push(p)
    }
  }
  for (const sub of ['components', 'hooks', 'content', 'pages', 'lib', 'gallery']) {
    const dir = join(ROOT, 'src', sub)
    if (existsSync(dir)) walk(dir)
  }
  const names = files.map((f) => f.split('/').pop()?.replace(/\.(tsx?|mts)$/, '') ?? '')
  for (let i = 0; i < names.length; i++) {
    if (!entries.has(names[i])) {
      problems.push(`unregistered component: ${files[i]} — add a COMPONENTS.md entry`)
    }
  }
  for (const entry of entries) {
    if (!names.includes(entry)) problems.push(`stale registry entry: ${entry} — no matching file`)
  }
  return problems
}

async function main() {
  const started = Date.now()

  // 1. discover + validate specs (throws on hard frontmatter violations)
  const { specs, warnings } = readSpecs()
  for (const w of warnings) console.warn(`⚠ ${w}`)
  if (ONLY && !specs.some((s) => s.slug === ONLY)) {
    throw new Error(`--only=${ONLY}: no such spec in ${SPECS_DIR}`)
  }

  // 2. pairing + entry contracts
  const deckDirs = discoverDeckDirs()
  for (const deck of deckDirs) {
    if (!existsSync(join(SPECS_DIR, deck))) {
      throw new Error(
        `orphan deck: ${join(ROOT, deck)} has no spec at ${join(SPECS_DIR, deck)} — ` +
          'the deck dir must be named after its spec dir',
      )
    }
    const html = readFileSync(join(ROOT, deck, 'index.html'), 'utf8')
    if (!html.includes('./src/main.tsx')) {
      throw new Error(
        `${deck}/index.html must reference its entry as "./src/main.tsx" (relative) — ` +
          'a root-relative "/src/..." breaks the shared dev server and subpath hosting',
      )
    }
  }

  // 3. registry parity
  const registryProblems = checkRegistry()
  for (const p of registryProblems) console.warn(`⚠ ${p}`)
  if (STRICT_REGISTRY && registryProblems.length > 0) {
    throw new Error(`--strict-registry: ${registryProblems.length} registry problem(s)`)
  }

  // 4. build the gallery (project-root index.html → dist/)
  if (!ONLY) rmSync(DIST, { recursive: true, force: true })
  mkdirSync(DIST, { recursive: true })
  await buildApp(ROOT, DIST, 'gallery (site root)')

  // 5. build the generic viewer once, if any target spec needs it
  const targets = specs.filter((s) => !ONLY || s.slug === ONLY)
  const needsViewer = targets.some((s) => !s.hasDeck)
  if (needsViewer) {
    await buildApp(join(ROOT, '_viewer'), VIEWER_TMP, 'spec viewer (md-only specs)')
  }

  // 6. one page per spec + raw markdown
  for (const spec of targets) {
    const out = join(DIST, spec.slug)
    if (ONLY) rmSync(out, { recursive: true, force: true })
    if (spec.hasDeck) {
      await buildApp(join(ROOT, spec.slug), out, `deck: ${spec.slug}  →  /${spec.slug}/`)
    } else {
      console.log(`\n▸ viewer: ${spec.slug}  →  /${spec.slug}/`)
      cpSync(VIEWER_TMP, out, { recursive: true })
      writeFileSync(
        join(out, 'spec.json'),
        `${JSON.stringify(
          { slug: spec.slug, title: spec.title, tagline: spec.tagline, docs: listSpecDocs(spec.slug) },
          null,
          2,
        )}\n`,
      )
    }
    for (const f of readdirSync(join(SPECS_DIR, spec.slug))) {
      if (f.endsWith('.md')) cpSync(join(SPECS_DIR, spec.slug, f), join(out, f))
    }
  }
  rmSync(VIEWER_TMP, { recursive: true, force: true })

  // 7. dist/index.json — the landing-page feed. Drafts are deliberately
  // excluded here (and from the sitemap); they still build and show muted in
  // the gallery.
  const feed = {
    generatedAt: new Date().toISOString(),
    specs: specs
      .filter((s) => s.status !== 'draft')
      .map(({ slug, title, tagline, date, month, dayLabel, tags, status, hasDeck }) => ({
        slug,
        title,
        tagline,
        date,
        month,
        dayLabel,
        tags,
        status,
        hasDeck,
        url: `/roadmap/${slug}/`,
      })),
  }
  writeFileSync(join(DIST, 'index.json'), `${JSON.stringify(feed, null, 2)}\n`)

  // 8. post-build contract checks
  if (!statSync(join(DIST, 'index.html')).size) throw new Error('gallery emitted empty index.html')
  for (const spec of specs) {
    const page = join(DIST, spec.slug, 'index.html')
    if (!ONLY || spec.slug === ONLY) {
      if (!existsSync(page)) throw new Error(`missing ${page} after build`)
    }
  }
  JSON.parse(readFileSync(join(DIST, 'index.json'), 'utf8'))

  const secs = ((Date.now() - started) / 1000).toFixed(1)
  const built = targets.map((s) => `${s.slug}${s.hasDeck ? '' : ' (viewer)'}`).join(', ')
  console.log(`\n✓ dist/ in ${secs}s — gallery + ${targets.length} spec(s): ${built || '(none)'}`)
}

main().catch((err) => {
  console.error(`\n✗ build failed: ${err instanceof Error ? err.message : err}`)
  process.exit(1)
})
