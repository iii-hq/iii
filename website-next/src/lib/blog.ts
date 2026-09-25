import "server-only"

import { readdirSync, readFileSync } from "node:fs"
import { basename, join, resolve } from "node:path"
import matter from "gray-matter"
import { cache } from "react"
import { readingTime } from "./markdown"

// Posts are the Astro site's markdown, read in place so both sites publish
// the same content while they coexist. Images referenced as
// `../../assets/blog/<slug>/file` are served from `public/blog/<slug>/file`.
const POSTS_DIR = resolve(process.cwd(), "../website/src/content/blog")

export type Post = {
  slug: string
  title: string
  description: string
  /** ISO date */
  date: string
  /** ISO date, when the post was revised */
  updated?: string
  author?: string
  tags: string[]
  /** public URL of the banner, when the post has one */
  image?: string
  /** minutes */
  readingTime: number
  /** markdown body, frontmatter stripped */
  body: string
}

const toIso = (v: unknown) => (v instanceof Date ? v.toISOString().slice(0, 10) : String(v ?? ""))

/** Frontmatter is typed by hand; " -- " is an em dash in disguise. Rendered text gets the real one. */
const smartDashes = (text: string) => text.replace(/\s--\s/g, " — ")

/** `../../assets/blog/<slug>/x.png` (as written in the markdown) → `/blog/<slug>/x.png`. */
export function publicImage(src: string) {
  const m = src.match(/assets\/blog\/(.+)$/)
  return m ? `/blog/${m[1]}` : src
}

function readPost(file: string): Post | null {
  const raw = readFileSync(join(POSTS_DIR, file), "utf8")
  const { data, content } = matter(raw)
  if (data.draft) return null
  const slug = basename(file).replace(/\.mdx?$/, "")
  return {
    slug,
    title: String(data.title ?? slug),
    description: smartDashes(String(data.description ?? "")),
    date: toIso(data.pubDate),
    updated: data.updatedDate ? toIso(data.updatedDate) : undefined,
    author: data.author ? String(data.author) : undefined,
    tags: Array.isArray(data.tags) ? data.tags.map(String) : [],
    image: data.ogImage ? publicImage(String(data.ogImage)) : undefined,
    readingTime: readingTime(content),
    body: content,
  }
}

/** Every published post, newest first. Read once per request: metadata and page share the result. */
export const getPosts = cache((): Post[] =>
  readdirSync(POSTS_DIR)
    .filter((f) => /\.mdx?$/.test(f))
    .map(readPost)
    .filter((p): p is Post => p !== null)
    .sort((a, b) => (a.date < b.date ? 1 : -1)),
)

export function getPost(slug: string): Post | null {
  return getPosts().find((p) => p.slug === slug) ?? null
}

const LONG_DATE = new Intl.DateTimeFormat("en-US", { month: "long", day: "numeric", year: "numeric", timeZone: "UTC" })

/** "May 7, 2026" from an ISO date. */
export function formatDate(iso: string) {
  return LONG_DATE.format(new Date(`${iso}T00:00:00Z`))
}
