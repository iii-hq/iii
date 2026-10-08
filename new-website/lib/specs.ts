import 'server-only'

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { cache } from 'react'

/**
 * The roadmap's specs, read the same way the live iii.dev reads them (website/src/lib/specs.ts): every folder in
 * <repo>/tech-specs with a README.md, its frontmatter for title, tagline, date, tags and status, newest first. A spec
 * has an interactive deck when website/roadmap/<slug>/src/App.tsx exists.
 */
const SPECS_DIR = resolve(process.cwd(), '../tech-specs')
const DECKS_DIR = resolve(process.cwd(), '../website/roadmap')

export type Spec = {
  slug: string
  title: string
  tagline: string
  /** ISO date, from frontmatter or the YYYY-MM-DD folder prefix */
  date: string
  tags: string[]
  status: 'live' | 'draft' | string
  /** the interactive deck on iii.dev, when the spec has one */
  deckUrl?: string
}

/** The small YAML subset spec frontmatter uses: `key: value`, quoted strings, and `[a, b]` lists. */
function frontmatter(source: string): Record<string, string | string[]> {
  const block = source.match(/^---\n([\s\S]*?)\n---/)?.[1]
  if (!block) return {}
  const data: Record<string, string | string[]> = {}
  for (const line of block.split('\n')) {
    const match = line.match(/^([A-Za-z_][\w-]*):\s*(.*)$/)
    if (!match) continue
    const [, key, raw] = match
    const value = raw.trim()
    if (value.startsWith('[') && value.endsWith(']')) {
      data[key] = value
        .slice(1, -1)
        .split(',')
        .map((item) => item.trim().replace(/^["']|["']$/g, ''))
        .filter(Boolean)
    } else {
      data[key] = value.replace(/^["']|["']$/g, '')
    }
  }
  return data
}

const firstH1 = (md: string) => md.match(/^#\s+(.+)$/m)?.[1]?.trim()

function readSpec(slug: string): Spec | null {
  const dir = join(SPECS_DIR, slug)
  const readme = join(dir, 'README.md')
  if (!statSync(dir).isDirectory() || !existsSync(readme)) return null
  const source = readFileSync(readme, 'utf8')
  const data = frontmatter(source)
  const str = (key: string) => (typeof data[key] === 'string' ? (data[key] as string) : '')
  return {
    slug,
    title: str('title') || firstH1(source) || slug,
    tagline: str('tagline'),
    date: str('date') || slug.match(/^(\d{4}-\d{2}-\d{2})/)?.[1] || '',
    tags: Array.isArray(data.tags) ? data.tags : [],
    status: str('status') || 'live',
    deckUrl: existsSync(join(DECKS_DIR, slug, 'src', 'App.tsx')) ? `https://iii.dev/roadmap/${slug}/deck/` : undefined,
  }
}

/** Every spec with a README, drafts included, newest first. */
export const getAllSpecs = cache((): Spec[] =>
  readdirSync(SPECS_DIR)
    .filter((name) => !name.startsWith('.') && statSync(join(SPECS_DIR, name)).isDirectory())
    .map(readSpec)
    .filter((spec): spec is Spec => spec !== null)
    .sort((a, b) => (a.date < b.date ? 1 : -1)),
)

const MONTHS = [
  'January',
  'February',
  'March',
  'April',
  'May',
  'June',
  'July',
  'August',
  'September',
  'October',
  'November',
  'December',
]
const parts = (iso: string) => iso.match(/^(\d{4})-(\d{2})(?:-(\d{2}))?$/)

/** "2026-06-29" → "June 2026" */
export function monthHeading(iso: string) {
  const m = parts(iso)
  return m ? `${MONTHS[Number(m[2]) - 1]} ${m[1]}` : iso
}

/** "2026-06-29" → "Jun 29" */
export function dayHeading(iso: string) {
  const m = parts(iso)
  return m?.[3] ? `${MONTHS[Number(m[2]) - 1].slice(0, 3)} ${Number(m[3])}` : iso
}

/** Where a spec card goes: live specs to their page on iii.dev, drafts to their folder on GitHub. */
export const specHref = (spec: Spec) =>
  spec.status === 'live'
    ? `https://iii.dev/roadmap/${spec.slug}/`
    : `https://github.com/iii-hq/iii/tree/main/tech-specs/${spec.slug}`
