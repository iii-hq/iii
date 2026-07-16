/**
 * manifest.mjs — spec discovery + frontmatter parsing.
 *
 * The single source of truth for "which tech specs exist and what are they
 * called". Consumed by:
 *   - build.mjs            (dist/index.json, viewer emission, contract checks)
 *   - vite.config.ts       (the `virtual:spec-manifest` module the gallery renders)
 *
 * Specs live at <repo>/tech-specs/<slug>/ and are MARKDOWN ONLY. Metadata is
 * YAML frontmatter at the top of each spec's README.md (title, tagline, date,
 * tags, status, featured) with derived fallbacks: date ← the dirname's
 * YYYY-MM-DD (or legacy YYYY-MM) prefix, title ← the README's first H1.
 * `slug` is NEVER a frontmatter field — the directory name is the identity
 * (folder = URL = deck dir), which kills the old manifest-vs-dirname drift
 * bug class by construction.
 *
 * Dates carry day precision so the gallery can render the roadmap timeline:
 * `month` groups entries ("2026 · june"), `dayLabel` marks each one ("jun 29",
 * null when a spec only has a month).
 *
 * Dependency-free by design (same approach as website/scripts/blog-posts.ts):
 * a minimal parser sized to our six fields instead of a YAML library.
 */

import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

// The roadmap project root (this file lives in <root>/scripts/). File-relative
// resolution is correct whenever the module runs from its real location (tsx
// scripts, the Vite plugin below, standalone copies in other repos) — but
// `astro build` BUNDLES imported modules, so import.meta.url then points into
// the build chunk dir. Detect that by checking for this very file, and fall
// back to the working directory (astro runs from the website package root).
const FILE_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
/** the roadmap project root */
export const ROOT = existsSync(join(FILE_ROOT, 'scripts', 'manifest.mjs'))
  ? FILE_ROOT
  : resolve(process.cwd(), 'roadmap')
/** the markdown-only spec tree, a sibling of website/ at the repo root */
export const SPECS_DIR = resolve(ROOT, '..', '..', 'tech-specs')

const FRONTMATTER_RE = /^---\r?\n([\s\S]*?)\r?\n---/
const KNOWN_KEYS = new Set(['title', 'tagline', 'date', 'tags', 'status', 'featured'])
const MONTH_NAMES = [
  'january',
  'february',
  'march',
  'april',
  'may',
  'june',
  'july',
  'august',
  'september',
  'october',
  'november',
  'december',
]

/** @param {string} raw */
export function parseFrontmatter(raw) {
  const match = raw.match(FRONTMATTER_RE)
  /** @type {Record<string, string | string[] | boolean>} */
  const fields = {}
  /** @type {string[]} */
  const unknown = []
  if (!match) return { fields, unknown, present: false }
  for (const line of match[1].split(/\r?\n/)) {
    const m = line.match(/^([A-Za-z_][\w-]*)\s*:\s*(.*?)\s*$/)
    if (!m) continue
    const key = m[1]
    let value = m[2]
    if (!KNOWN_KEYS.has(key)) {
      unknown.push(key)
      continue
    }
    if (key === 'tags') {
      const inner = value.replace(/^\[/, '').replace(/\]$/, '')
      fields.tags = inner
        .split(',')
        .map((t) => t.trim().replace(/^['"]|['"]$/g, ''))
        .filter(Boolean)
      continue
    }
    if (key === 'featured') {
      fields.featured = value === 'true'
      continue
    }
    if (
      (value.startsWith("'") && value.endsWith("'")) ||
      (value.startsWith('"') && value.endsWith('"'))
    ) {
      value = value.slice(1, -1)
    }
    fields[key] = value
  }
  // `slug` in frontmatter is the drift bug the dirname-identity rule exists to
  // prevent — surface it loudly rather than silently ignoring it.
  if (/^slug\s*:/m.test(match[1])) {
    throw new Error(
      'frontmatter declares `slug` — the directory name IS the slug; remove the field',
    )
  }
  return { fields, unknown, present: true }
}

/** @param {string} md */
export function stripFrontmatter(md) {
  return md.replace(FRONTMATTER_RE, '').replace(/^\s+/, '')
}

/** @param {string} md */
export function firstHeading(md) {
  const m = stripFrontmatter(md).match(/^#\s+(.+)$/m)
  return m ? m[1].trim() : null
}

/** @param {string} md */
function firstParagraph(md) {
  for (const block of stripFrontmatter(md).split(/\r?\n\r?\n/)) {
    const line = block.trim().replace(/\s+/g, ' ')
    if (!line || line.startsWith('#') || line.startsWith('```') || line.startsWith('|')) continue
    return line
  }
  return null
}

/** "2026-06-29" or "2026-06" → "2026 · june" (the timeline's month group) */
export function monthLabel(date) {
  const m = date?.match(/^(\d{4})-(\d{2})(?:-\d{2})?$/)
  if (!m) return date ?? ''
  const name = MONTH_NAMES[Number(m[2]) - 1]
  return name ? `${m[1]} · ${name}` : date
}

/** "2026-06-29" → "jun 29"; null when the date has no day component */
export function dayLabel(date) {
  const m = date?.match(/^\d{4}-(\d{2})-(\d{2})$/)
  if (!m) return null
  const name = MONTH_NAMES[Number(m[1]) - 1]
  return name ? `${name.slice(0, 3)} ${Number(m[2])}` : null
}

/**
 * Non-README markdown files of a spec, viewer/spec-page ordering: README
 * first, then alphabetical; review notes (`*-review-*`) and underscore-prefixed
 * scratch files excluded — same filter the deck SpecPage applies.
 * @param {string} slug
 */
export function listSpecDocs(slug) {
  const dir = join(SPECS_DIR, slug)
  const files = readdirSync(dir)
    .filter((f) => f.endsWith('.md'))
    .filter((f) => !/-review-/.test(f) && !f.startsWith('_'))
    .sort((a, b) => (a === 'README.md' ? -1 : b === 'README.md' ? 1 : a.localeCompare(b)))
  return files.map((file) => {
    const raw = readFileSync(join(dir, file), 'utf8')
    return { file, label: firstHeading(raw) ?? file.replace(/\.md$/, '') }
  })
}

/**
 * Read every spec in tech-specs/: parse + validate frontmatter, apply
 * fallbacks, detect decks, and sort gallery-style (featured first, then date
 * desc, then title). Returns { specs, warnings }; throws on hard contract
 * violations (a `slug` frontmatter field).
 */
export function readSpecs() {
  /** @type {string[]} */
  const warnings = []
  const specs = []
  if (!existsSync(SPECS_DIR)) return { specs, warnings: [`no spec tree at ${SPECS_DIR}`] }

  for (const entry of readdirSync(SPECS_DIR, { withFileTypes: true })) {
    if (!entry.isDirectory() || entry.name.startsWith('.') || entry.name.startsWith('_')) continue
    const slug = entry.name
    const readmePath = join(SPECS_DIR, slug, 'README.md')
    if (!existsSync(readmePath)) {
      warnings.push(`${slug}: no README.md — spec not listed`)
      continue
    }
    const raw = readFileSync(readmePath, 'utf8')
    let parsed
    try {
      parsed = parseFrontmatter(raw)
    } catch (err) {
      throw new Error(`${slug}/README.md: ${err instanceof Error ? err.message : err}`)
    }
    const { fields, unknown } = parsed
    for (const key of unknown) warnings.push(`${slug}: unknown frontmatter key \`${key}\``)
    if (!/^\d{4}-\d{2}-\d{2}-[a-z][a-z0-9-]*$/.test(slug)) {
      warnings.push(`${slug}: dirname does not match YYYY-MM-DD-<slug> convention`)
    }

    const title = typeof fields.title === 'string' ? fields.title : firstHeading(raw)
    if (!title) warnings.push(`${slug}: no title frontmatter and no H1 — falling back to slug`)

    const tagline = typeof fields.tagline === 'string' ? fields.tagline : firstParagraph(raw)
    if (typeof fields.tagline !== 'string') {
      warnings.push(`${slug}: no tagline frontmatter — using the README's first paragraph`)
    }

    const dirDate = slug.match(/^(\d{4}-\d{2}(?:-\d{2})?)/)?.[1] ?? null
    let date = typeof fields.date === 'string' ? fields.date : null
    if (date && !/^\d{4}-(0[1-9]|1[0-2])(-(0[1-9]|[12][0-9]|3[01]))?$/.test(date)) {
      warnings.push(`${slug}: invalid date \`${date}\` — falling back to dirname prefix`)
      date = null
    }
    if (date && dirDate && date !== dirDate) {
      warnings.push(`${slug}: frontmatter date ${date} ≠ dirname prefix ${dirDate}`)
    }
    date = date ?? dirDate
    if (!date) {
      warnings.push(`${slug}: no date derivable — spec not listed`)
      continue
    }

    let status = typeof fields.status === 'string' ? fields.status : 'live'
    if (status !== 'live' && status !== 'draft') {
      warnings.push(`${slug}: invalid status \`${status}\` — treating as live`)
      status = 'live'
    }

    let tags = Array.isArray(fields.tags) ? fields.tags : []
    if (tags.length > 4) {
      warnings.push(`${slug}: ${tags.length} tags — keeping the first 4`)
      tags = tags.slice(0, 4)
    }

    specs.push({
      slug,
      title: title ?? slug,
      tagline: tagline ?? '',
      date,
      month: monthLabel(date),
      dayLabel: dayLabel(date),
      tags,
      status,
      featured: fields.featured === true,
      // a deck is its content layer's React entry — decks render as Astro
      // islands (src/DeckHost.tsx), so there is no per-deck index.html
      hasDeck: existsSync(join(ROOT, slug, 'src', 'App.tsx')),
    })
  }

  specs.sort(
    (a, b) =>
      Number(b.featured) - Number(a.featured) ||
      b.date.localeCompare(a.date) ||
      a.title.localeCompare(b.title),
  )
  return { specs, warnings }
}

/**
 * Vite plugin resolving `virtual:spec-manifest` to the live spec list — the
 * gallery's data source. No generated file in the tree, so nothing to commit
 * and nothing to conflict on; dev picks up frontmatter edits via the watcher.
 */
export function specManifestPlugin() {
  const VIRTUAL_ID = 'virtual:spec-manifest'
  const RESOLVED_ID = `\0${VIRTUAL_ID}`
  return {
    name: 'spec-manifest',
    resolveId(/** @type {string} */ id) {
      return id === VIRTUAL_ID ? RESOLVED_ID : undefined
    },
    load(/** @type {string} */ id) {
      if (id !== RESOLVED_ID) return undefined
      const { specs, warnings } = readSpecs()
      for (const w of warnings) this.warn(`[spec-manifest] ${w}`)
      return `export const SPECS = ${JSON.stringify(specs, null, 2)}\n`
    },
    configureServer(/** @type {import('vite').ViteDevServer} */ server) {
      server.watcher.add(SPECS_DIR)
      const invalidate = (/** @type {string} */ file) => {
        if (!file.startsWith(SPECS_DIR)) return
        const mod = server.moduleGraph.getModuleById(RESOLVED_ID)
        if (mod) server.moduleGraph.invalidateModule(mod)
        server.ws.send({ type: 'full-reload' })
      }
      server.watcher.on('change', invalidate)
      server.watcher.on('add', invalidate)
      server.watcher.on('unlink', invalidate)
    },
  }
}
