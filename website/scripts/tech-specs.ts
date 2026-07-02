import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export interface TechSpec {
  slug: string
  title: string
  /**
   * "YYYY-MM-DD" (legacy specs may carry month-only "YYYY-MM") — frontmatter
   * `date`, falling back to the dirname prefix
   */
  date: string
  status: 'live' | 'draft'
}

const SPECS_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../tech-specs')

// Same minimal frontmatter approach as blog-posts.ts: pull the YAML block and
// parse only the keys the sitemap needs — no YAML dependency. The full parser
// (tags, featured, fallbacks) lives in website/presentations/scripts/manifest.mjs;
// this one is deliberately tiny and read-only.
const FRONTMATTER_RE = /^---\r?\n([\s\S]*?)\r?\n---/

function parseFrontmatter(raw: string): Record<string, string> {
  const match = raw.match(FRONTMATTER_RE)
  if (!match) return {}
  const out: Record<string, string> = {}
  for (const line of match[1].split(/\r?\n/)) {
    const m = line.match(/^([A-Za-z_][\w-]*)\s*:\s*(.*?)\s*$/)
    if (!m) continue
    let value = m[2]
    if ((value.startsWith("'") && value.endsWith("'")) || (value.startsWith('"') && value.endsWith('"'))) {
      value = value.slice(1, -1)
    }
    out[m[1]] = value
  }
  return out
}

function firstHeading(md: string): string | undefined {
  const m = md.replace(FRONTMATTER_RE, '').match(/^#\s+(.+)$/m)
  return m ? m[1].trim() : undefined
}

/** Every published spec in tech-specs/, newest first. Missing dir → []. */
export async function readTechSpecs(dir = SPECS_DIR): Promise<TechSpec[]> {
  let entries: { name: string; isDirectory(): boolean }[]
  try {
    entries = await fs.readdir(dir, { withFileTypes: true })
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return []
    throw error
  }

  const specs: TechSpec[] = []
  for (const entry of entries) {
    if (!entry.isDirectory() || entry.name.startsWith('.') || entry.name.startsWith('_')) continue
    const slug = entry.name
    let raw: string
    try {
      raw = await fs.readFile(path.join(dir, slug, 'README.md'), 'utf8')
    } catch {
      continue // no README — the spec is not listed anywhere
    }
    const fm = parseFrontmatter(raw)
    const dirDate = slug.match(/^(\d{4}-\d{2}(?:-\d{2})?)/)?.[1]
    const date = /^\d{4}-(0[1-9]|1[0-2])(-(0[1-9]|[12][0-9]|3[01]))?$/.test(fm.date ?? '') ? fm.date : dirDate
    if (!date) continue
    specs.push({
      slug,
      title: fm.title ?? firstHeading(raw) ?? slug,
      date,
      status: fm.status === 'draft' ? 'draft' : 'live',
    })
  }
  specs.sort((a, b) => (a.date === b.date ? a.slug.localeCompare(b.slug) : b.date.localeCompare(a.date)))
  return specs
}
