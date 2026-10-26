import 'server-only'

import { readdirSync, readFileSync } from 'node:fs'
import { basename, join, resolve } from 'node:path'
import matter from 'gray-matter'
import { cache } from 'react'
import { readingTime, wordCount } from './markdown'

// Posts are markdown files in content/blog (the filename is the URL slug).
// Images referenced as `../../assets/blog/<slug>/file` are served from
// `public/blog/<slug>/file`.
const POSTS_DIR = resolve(process.cwd(), 'content/blog')

export type Faq = { q: string; a: string }

export type Post = {
  slug: string
  title: string
  /** the `<title>` and share-card title, when it should differ from the heading on the page */
  seoTitle?: string
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
  /** words in the body, for the article's structured data */
  wordCount: number
  /** questions the post answers, emitted as FAQPage structured data */
  faq: Faq[]
  /** markdown body, frontmatter stripped */
  body: string
}

const toIso = (v: unknown) => (v instanceof Date ? v.toISOString().slice(0, 10) : String(v ?? ''))

/** Frontmatter is typed by hand; " -- " is an em dash in disguise. Rendered text gets the real one. */
const smartDashes = (text: string) => text.replace(/\s--\s/g, ' — ')

/** `../../assets/blog/<slug>/x.png` (as written in the markdown) → `/blog/<slug>/x.png`. */
export function publicImage(src: string) {
  const m = src.match(/assets\/blog\/(.+)$/)
  return m ? `/blog/${m[1]}` : src
}

/** The `faq` frontmatter list: each entry needs a non-empty `q` and `a`; anything else is dropped. */
function readFaq(value: unknown): Faq[] {
  if (!Array.isArray(value)) return []
  return value.flatMap((item) => {
    const q = typeof item?.q === 'string' ? item.q.trim() : ''
    const a = typeof item?.a === 'string' ? item.a.trim() : ''
    return q && a ? [{ q, a }] : []
  })
}

function readPost(file: string): Post | null {
  const raw = readFileSync(join(POSTS_DIR, file), 'utf8')
  const { data, content } = matter(raw)
  if (data.draft) return null
  const slug = basename(file).replace(/\.mdx?$/, '')
  return {
    slug,
    title: String(data.title ?? slug),
    seoTitle: data.seoTitle ? String(data.seoTitle) : undefined,
    description: smartDashes(String(data.description ?? '')),
    date: toIso(data.pubDate),
    updated: data.updatedDate ? toIso(data.updatedDate) : undefined,
    author: data.author ? String(data.author) : undefined,
    tags: Array.isArray(data.tags) ? data.tags.map(String) : [],
    image: data.ogImage ? publicImage(String(data.ogImage)) : undefined,
    readingTime: readingTime(content),
    wordCount: wordCount(content),
    faq: readFaq(data.faq),
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

const LONG_DATE = new Intl.DateTimeFormat('en-US', { month: 'long', day: 'numeric', year: 'numeric', timeZone: 'UTC' })

/** "May 7, 2026" from an ISO date. */
export function formatDate(iso: string) {
  return LONG_DATE.format(new Date(`${iso}T00:00:00Z`))
}
