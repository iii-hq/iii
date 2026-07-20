// Roadmap contract checks, run before `astro build` (see the package build
// script) — ported from the old roadmap/build.mjs when the decks became
// Astro routes:
//   - spec frontmatter warnings (readSpecs throws on a hard `slug` violation)
//   - orphan decks: roadmap/<dir>/src/App.tsx without a tech-specs/<dir>/
//   - COMPONENTS.md registry parity for the shared library (warn by default;
//     --strict makes registry drift fatal)
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROOT, readSpecs, SPECS_DIR } from '../roadmap/scripts/manifest.mjs'

const STRICT = process.argv.includes('--strict')
let failed = false

const { specs, warnings } = readSpecs()
for (const w of warnings) console.warn(`⚠ ${w}`)

// orphan decks — the deck dir must be named after its spec dir
const deckDirs = readdirSync(ROOT, { withFileTypes: true })
  .filter((e) => e.isDirectory() && !e.name.startsWith('.') && !e.name.startsWith('_'))
  .map((e) => e.name)
  .filter((name) => existsSync(join(ROOT, name, 'src', 'App.tsx')))
  .sort()
for (const deck of deckDirs) {
  if (!existsSync(join(SPECS_DIR, deck))) {
    console.error(`✗ orphan deck: roadmap/${deck} has no spec at tech-specs/${deck}`)
    failed = true
  }
}

// registry parity — every shared src file needs a `### <basename>` entry
const registryPath = join(ROOT, 'COMPONENTS.md')
const registry = existsSync(registryPath) ? readFileSync(registryPath, 'utf8') : ''
const entries = new Set([...registry.matchAll(/^### (\S+)$/gm)].map((m) => m[1]))
const skip = new Set(['main.tsx', 'App.tsx', 'vite-env.d.ts', 'index.css'])
const files: string[] = []
const walk = (dir: string) => {
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
const names = files.map(
  (f) =>
    f
      .split('/')
      .pop()
      ?.replace(/\.(tsx?|mts)$/, '') ?? '',
)
const registryProblems: string[] = []
for (let i = 0; i < names.length; i++) {
  if (!entries.has(names[i])) registryProblems.push(`unregistered component: ${files[i]} — add a COMPONENTS.md entry`)
}
for (const entry of entries) {
  if (!names.includes(entry)) registryProblems.push(`stale registry entry: ${entry} — no matching file`)
}
for (const p of registryProblems) console.warn(`⚠ ${p}`)
if (STRICT && registryProblems.length > 0) failed = true

if (failed) {
  console.error('✗ roadmap contract violations — see above')
  process.exit(1)
}
console.log(`✓ roadmap contracts: ${specs.length} spec(s), ${deckDirs.length} deck(s)`)
