#!/usr/bin/env node
/**
 * build.mjs — the orchestrator that turns this folder into ONE static site.
 *
 * Output layout (dist/):
 *   /                 the gallery (index of every deck)        <- _gallery/dist
 *   /<slug>/          one built presentation per spec           <- <slug>/presentation/dist
 *
 * It builds the gallery, then discovers every `<dir>/presentation/` sibling,
 * builds each, and copies its output to `dist/<dir>/`. The directory name IS
 * the slug — it must match the `slug` field in _gallery/src/content/
 * presentations.ts (that is the contract a gallery card links through).
 *
 * Every sub-build is a standalone pnpm project (`--ignore-workspace`), exactly
 * like running `/presentation` produces — nothing here pulls a parent workspace
 * graph. Presentations use `base: './'`, so their relative asset paths resolve
 * correctly when served from a `/<slug>/` subpath.
 *
 * Usage:
 *   node build.mjs                 install + build everything (what Vercel runs)
 *   node build.mjs --skip-install  reuse existing node_modules (fast local rebuild)
 *   node build.mjs --only=<slug>   build only the gallery + one presentation
 */

import { execFileSync } from 'node:child_process'
import {
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  rmSync,
} from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = dirname(fileURLToPath(import.meta.url))
const DIST = join(ROOT, 'dist')
const GALLERY = join(ROOT, '_gallery')

const args = process.argv.slice(2)
const SKIP_INSTALL = args.includes('--skip-install')
const ONLY = args.find((a) => a.startsWith('--only='))?.slice('--only='.length)

// directories that are never a presentation
const IGNORE = new Set(['_gallery', 'dist', 'node_modules', '.git', '.vercel'])

function run(cmd, cmdArgs, cwd) {
  execFileSync(cmd, cmdArgs, { cwd, stdio: 'inherit' })
}

function buildProject(dir, label) {
  console.log(`\n▸ ${label}`)
  // --no-frozen-lockfile so a slightly-stale committed lockfile self-heals
  // instead of hard-failing the deploy. CI (Vercel sets CI=true) otherwise
  // defaults to frozen, which aborts on any lockfile/package.json drift — a
  // confusing "builds locally, fails on Vercel" trap for future decks.
  if (!SKIP_INSTALL)
    run('pnpm', ['install', '--ignore-workspace', '--no-frozen-lockfile'], dir)
  run('pnpm', ['build'], dir)
}

/** every sibling dir that holds a buildable presentation, sorted, slug = name */
function discoverPresentations() {
  return readdirSync(ROOT, { withFileTypes: true })
    .filter((e) => e.isDirectory() && !IGNORE.has(e.name))
    .map((e) => e.name)
    .filter((name) => existsSync(join(ROOT, name, 'presentation', 'package.json')))
    .filter((name) => !ONLY || name === ONLY)
    .sort()
}

function main() {
  const started = Date.now()
  rmSync(DIST, { recursive: true, force: true })
  mkdirSync(DIST, { recursive: true })

  // 1. the gallery becomes the site root
  buildProject(GALLERY, 'gallery (site root)')
  cpSync(join(GALLERY, 'dist'), DIST, { recursive: true })

  // 2. each presentation lands at /<slug>/
  const slugs = discoverPresentations()
  for (const slug of slugs) {
    const proj = join(ROOT, slug, 'presentation')
    buildProject(proj, `presentation: ${slug}  ->  /${slug}/`)
    cpSync(join(proj, 'dist'), join(DIST, slug), { recursive: true })
  }

  const secs = ((Date.now() - started) / 1000).toFixed(1)
  console.log(
    `\n✓ built dist/ in ${secs}s — gallery + ${slugs.length} ` +
      `presentation${slugs.length === 1 ? '' : 's'}: ${slugs.join(', ') || '(none)'}`,
  )
  if (slugs.length === 0) {
    console.log(
      '  (no <dir>/presentation/ found yet — run /presentation <spec-dir> to add one)',
    )
  }
}

main()
