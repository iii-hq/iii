import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export interface BlogPost {
  slug: string
  title: string
  pubDate: Date
  updatedDate?: Date
  draft: boolean
}

const BLOG_CONTENT_DIR = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../blog/src/content/blog',
)

// Minimal frontmatter reader sized to our schema (title, pubDate, updatedDate,
// draft). We pull the YAML block between leading `---` fences and parse only
// the four keys we need — keeping the website build dependency-free instead
// of pulling in a YAML lib for a handful of fields.
const FRONTMATTER_RE = /^---\r?\n([\s\S]*?)\r?\n---/

function parseFrontmatter(raw: string): Record<string, string> {
  const match = raw.match(FRONTMATTER_RE)
  if (!match) return {}
  const out: Record<string, string> = {}
  for (const line of match[1].split(/\r?\n/)) {
    const m = line.match(/^([A-Za-z_][\w-]*)\s*:\s*(.*?)\s*$/)
    if (!m) continue
    let value = m[2]
    if (
      (value.startsWith("'") && value.endsWith("'")) ||
      (value.startsWith('"') && value.endsWith('"'))
    ) {
      value = value.slice(1, -1)
    }
    out[m[1]] = value
  }
  return out
}

function parseDate(value: string | undefined): Date | undefined {
  if (!value) return undefined
  const d = new Date(value)
  return Number.isNaN(d.valueOf()) ? undefined : d
}

export async function readBlogPosts(dir = BLOG_CONTENT_DIR): Promise<BlogPost[]> {
  let entries: string[]
  try {
    entries = await fs.readdir(dir)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return []
    throw error
  }

  const posts: BlogPost[] = []
  for (const entry of entries) {
    if (!/\.(md|mdx)$/.test(entry)) continue
    const raw = await fs.readFile(path.join(dir, entry), 'utf8')
    const fm = parseFrontmatter(raw)
    const pubDate = parseDate(fm.pubDate)
    if (!pubDate) continue
    posts.push({
      slug: entry.replace(/\.(md|mdx)$/, ''),
      title: fm.title ?? entry,
      pubDate,
      updatedDate: parseDate(fm.updatedDate),
      draft: fm.draft === 'true',
    })
  }
  posts.sort((a, b) => b.pubDate.valueOf() - a.pubDate.valueOf())
  return posts
}
