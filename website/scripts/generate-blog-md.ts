import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { type BlogPost, readBlogPosts } from './blog-posts'
import { SITE_ORIGIN } from './routes'

const WEBSITE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const BLOG_DIST = path.resolve(WEBSITE_ROOT, '../blog/dist')
const BLOG_CONTENT = path.resolve(WEBSITE_ROOT, '../blog/src/content/blog')

const IMAGE_RE = /!\[([^\]]*)\]\(\.\.\/\.\.\/assets\/blog\/[^/]+\/([^)]+)\)/g

function escapeMarkdownLinkText(text: string): string {
  return text.replace(/\\/g, '\\\\').replace(/\[/g, '\\[').replace(/\]/g, '\\]')
}

export function formatBlogPostLinkLine(post: BlogPost): string {
  const title = escapeMarkdownLinkText(post.title)
  const url = `${SITE_ORIGIN}/blog/${post.slug}.md`
  const description = post.description.replace(/\s+/g, ' ').trim()
  return `- [${title}](${url}) — ${description}`
}

async function listAstroAssets(distDir: string): Promise<string[]> {
  try {
    return await fs.readdir(path.join(distDir, '_astro'))
  } catch {
    return []
  }
}

const POST_ASSET_RE = /\/blog\/_astro\/([^"'\s)]+)/g

async function listPostAstroAssets(distDir: string, slug: string): Promise<string[] | null> {
  let html: string
  try {
    html = await fs.readFile(path.join(distDir, slug, 'index.html'), 'utf8')
  } catch {
    return null
  }
  const assets = new Set<string>()
  for (const match of html.matchAll(POST_ASSET_RE)) assets.add(match[1])
  return assets.size > 0 ? [...assets] : null
}

function resolveAssetUrl(filename: string, astroAssets: string[]): string | null {
  const stem = path.basename(filename, path.extname(filename))
  const match = astroAssets.find((file) => file.startsWith(`${stem}.`))
  return match ? `${SITE_ORIGIN}/blog/_astro/${match}` : null
}

export function rewriteBlogImagePaths(body: string, astroAssets: string[]): string {
  return body.replace(IMAGE_RE, (full, alt: string, filename: string) => {
    const url = resolveAssetUrl(filename, astroAssets)
    return url ? `![${alt}](${url})` : full
  })
}

export async function exportBlogMarkdown(
  contentDir = BLOG_CONTENT,
  distDir = BLOG_DIST,
): Promise<number> {
  const posts = (await readBlogPosts(contentDir)).filter((post) => !post.draft)
  if (posts.length === 0) {
    throw new Error(`no publishable blog posts found in ${contentDir} — refusing to export an empty blog`)
  }
  const astroAssets = await listAstroAssets(distDir)

  await Promise.all(
    posts.map(async (post) => {
      if (post.slug === 'index') {
        throw new Error(`blog post slug "index" collides with the generated blog/index.md`)
      }
      const sourcePath = path.join(contentDir, post.sourceFile)
      const raw = await fs.readFile(sourcePath, 'utf8')
      const postAssets = (await listPostAstroAssets(distDir, post.slug)) ?? astroAssets
      const exported = rewriteBlogImagePaths(raw, postAssets)
      await fs.writeFile(path.join(distDir, `${post.slug}.md`), exported, 'utf8')
    }),
  )

  const indexLines = posts.map(formatBlogPostLinkLine)
  await fs.writeFile(
    path.join(distDir, 'index.md'),
    [
      '# iii blog',
      '',
      'Architecture posts and examples for coding agents. Read as markdown.',
      '',
      `- [Blog home (HTML)](${SITE_ORIGIN}/blog/)`,
      '',
      ...indexLines,
      '',
    ].join('\n'),
    'utf8',
  )

  return posts.length
}

/** Markdown bullet list for llms.txt / AGENTS.md — links to agent-readable .md URLs. */
export async function buildBlogLinksSection(): Promise<string> {
  const posts = (await readBlogPosts()).filter((post) => !post.draft)
  if (posts.length === 0) return ''

  return [
    '## Blog (knowledge base for coding agents)',
    '',
    'Long-form architecture posts, harness patterns, and worked examples. Fetch as markdown:',
    '',
    `- [Blog index](${SITE_ORIGIN}/blog/index.md) — all posts`,
    ...posts.map(formatBlogPostLinkLine),
    '',
  ].join('\n')
}

async function main() {
  const count = await exportBlogMarkdown()
  console.log(`exported ${count} blog post(s) + index.md to ${path.relative(WEBSITE_ROOT, BLOG_DIST)}`)
}

const isMain = import.meta.url === pathToFileURL(path.resolve(process.argv[1] ?? '')).href
if (isMain) {
  main().catch((err) => {
    console.error(err)
    process.exitCode = 1
  })
}
