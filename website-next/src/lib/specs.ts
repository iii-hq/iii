import "server-only"

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs"
import { join, resolve } from "node:path"
import matter from "gray-matter"
import { cache } from "react"
import { site } from "./site"

// Tech specs live at <repo>/tech-specs/<slug>/ as markdown only. The directory
// name is the slug (folder = URL). Interactive decks are still built by the
// Astro site under website/roadmap/<slug>/src/App.tsx; until they move, a
// spec that has one links out to the live deck.
const SPECS_DIR = resolve(process.cwd(), "../tech-specs")
const DECKS_DIR = resolve(process.cwd(), "../website/roadmap")

export type SpecDoc = {
  /** file name, e.g. "README.md" */
  file: string
  /** first H1 or the file name */
  title: string
  markdown: string
}

export type Spec = {
  slug: string
  title: string
  tagline: string
  /** ISO date, from frontmatter or the YYYY-MM-DD dirname prefix */
  date: string
  tags: string[]
  status: "live" | "draft" | string
  featured: boolean
  /** the README body, frontmatter stripped */
  body: string
  /** every markdown file in the spec folder, README first */
  docs: SpecDoc[]
  /** the interactive deck on the current site, when the spec has one */
  deckUrl?: string
}

const firstH1 = (md: string) => md.match(/^#\s+(.+)$/m)?.[1]?.trim()
const toIso = (v: unknown) => (v instanceof Date ? v.toISOString().slice(0, 10) : String(v ?? ""))

function readSpec(slug: string): Spec | null {
  const dir = join(SPECS_DIR, slug)
  const readme = join(dir, "README.md")
  if (!statSync(dir).isDirectory() || !existsSync(readme)) return null
  const { data, content } = matter(readFileSync(readme, "utf8"))
  const dateFromDir = slug.match(/^(\d{4}-\d{2}-\d{2})/)?.[1]
  const docs: SpecDoc[] = readdirSync(dir)
    .filter((f) => f.endsWith(".md"))
    .sort((a, b) => (a === "README.md" ? -1 : b === "README.md" ? 1 : a.localeCompare(b)))
    .map((file) => {
      const md = file === "README.md" ? content : matter(readFileSync(join(dir, file), "utf8")).content
      return { file, title: firstH1(md) ?? file.replace(/\.md$/, ""), markdown: md }
    })
  const hasDeck = existsSync(join(DECKS_DIR, slug, "src", "App.tsx"))
  return {
    slug,
    title: String(data.title ?? firstH1(content) ?? slug),
    tagline: String(data.tagline ?? ""),
    date: data.date ? toIso(data.date) : (dateFromDir ?? ""),
    tags: Array.isArray(data.tags) ? data.tags.map(String) : [],
    status: String(data.status ?? "live"),
    featured: data.featured === true,
    body: content,
    docs,
    deckUrl: hasDeck ? `${site.url}/roadmap/${slug}/` : undefined,
  }
}

/**
 * Every spec with a README, drafts included, newest first. The roadmap sheet
 * shows drafts as upcoming work. Read once per request: metadata, page and
 * pager share the result.
 */
export const getAllSpecs = cache((): Spec[] =>
  readdirSync(SPECS_DIR)
    .filter((name) => !name.startsWith(".") && statSync(join(SPECS_DIR, name)).isDirectory())
    .map(readSpec)
    .filter((s): s is Spec => s !== null)
    .sort((a, b) => (a.date < b.date ? 1 : -1)),
)

/** Every published (non-draft) spec, newest first. These are the ones with a page. */
export function getSpecs(): Spec[] {
  return getAllSpecs().filter((s) => s.status !== "draft")
}

export function getSpec(slug: string): Spec | null {
  return getSpecs().find((s) => s.slug === slug) ?? null
}
