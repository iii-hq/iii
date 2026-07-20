import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { BLOG_CONTENT_DIR, type BlogPost, readBlogPosts } from './blog-posts'
import { SITE_ORIGIN } from './routes'

const WEBSITE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
// The export runs after `astro build` (see the package build script), so the
// built site is the source of truth for asset URLs and the output lands next
// to the built /blog pages.
const BLOG_DIST = path.join(WEBSITE_ROOT, 'dist', 'blog')

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

// Absolute asset paths as they appear in the built HTML (e.g.
// /_astro/banner.CKx2.png). Captured from each post's own page so the export
// never guesses where the bundler put an image or what it hashed it to.
const BUILT_ASSET_PATH_RE = /["'(](\/[^"'()\s]*_astro\/[^"'()\s]+)["')]/g

export async function listPostAssetPaths(distDir: string, slug: string): Promise<string[]> {
  const htmlPath = path.join(distDir, slug, 'index.html')
  const html = await fs.readFile(htmlPath, 'utf8').catch(() => {
    throw new Error(`generate-blog-md: ${htmlPath} missing — run \`astro build\` first`)
  })
  const assets = new Set<string>()
  for (const match of html.matchAll(BUILT_ASSET_PATH_RE)) assets.add(match[1])
  return [...assets]
}

function resolveAssetUrl(filename: string, assetPaths: string[]): string | null {
  const stem = path.basename(filename, path.extname(filename))
  const match = assetPaths.find((assetPath) => path.basename(assetPath).startsWith(`${stem}.`))
  return match ? `${SITE_ORIGIN}${match}` : null
}

export function rewriteBlogImagePaths(body: string, assetPaths: string[]): string {
  return body.replace(IMAGE_RE, (full, alt: string, filename: string) => {
    const url = resolveAssetUrl(filename, assetPaths)
    return url ? `![${alt}](${url})` : full
  })
}

export async function exportBlogMarkdown(
  contentDir = BLOG_CONTENT_DIR,
  distDir = BLOG_DIST,
): Promise<number> {
  const posts = (await readBlogPosts(contentDir)).filter((post) => !post.draft)
  if (posts.length === 0) {
    throw new Error(`no publishable blog posts found in ${contentDir} — refusing to export an empty blog`)
  }

  await Promise.all(
    posts.map(async (post) => {
      if (post.slug === 'index') {
        throw new Error(`blog post slug "index" collides with the generated blog/index.md`)
      }
      const sourcePath = path.join(contentDir, post.sourceFile)
      const raw = await fs.readFile(sourcePath, 'utf8')
      const assetPaths = await listPostAssetPaths(distDir, post.slug)
      const exported = rewriteBlogImagePaths(raw, assetPaths)
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
