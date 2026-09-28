import fs from "node:fs/promises"
import path from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
import { BLOG_CONTENT_DIR, type BlogPost, readBlogPosts } from "./blog-posts"
import { SITE_ORIGIN } from "./routes"

const WEBSITE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
// Runs after `next build` + finalize-dist (see the package build script): the
// .md twins land next to the built /blog pages, dist/blog/<slug>/index.html.
const BLOG_DIST = path.join(WEBSITE_ROOT, "dist", "blog")
// Post images are written as `../../assets/blog/<slug>/<file>` (the historical
// path) and served verbatim from public/blog/<slug>/<file>; see src/lib/blog.ts.
const BLOG_PUBLIC_DIR = path.join(WEBSITE_ROOT, "public", "blog")

// Every `../../assets/blog/<slug>/<file>` reference: inline images, the
// frontmatter `ogImage`, raw <img src> in .mdx. Stops at whitespace, quotes and
// the closing paren so a trailing markdown title (`"..."`) is left alone.
const ASSET_REF_RE = /\.\.\/\.\.\/assets\/blog\/([^/\s"')]+)\/([^\s"')]+)/g

function escapeMarkdownLinkText(text: string): string {
  return text.replace(/\\/g, "\\\\").replace(/\[/g, "\\[").replace(/\]/g, "\\]")
}

export function formatBlogPostLinkLine(post: BlogPost): string {
  const title = escapeMarkdownLinkText(post.title)
  const url = `${SITE_ORIGIN}/blog/${post.slug}.md`
  const description = post.description.replace(/\s+/g, " ").trim()
  return `- [${title}](${url}) — ${description}`
}

/** `../../assets/blog/<slug>/<file>` → `https://iii.dev/blog/<slug>/<file>` (the public/ copy). */
export function publicImageUrl(slug: string, file: string): string {
  return `${SITE_ORIGIN}/blog/${slug}/${file}`
}

/** The `<slug>/<file>` pairs a post body references. */
export function listReferencedImages(body: string): { slug: string; file: string }[] {
  const seen = new Set<string>()
  const refs: { slug: string; file: string }[] = []
  for (const m of body.matchAll(ASSET_REF_RE)) {
    const key = `${m[1]}/${m[2]}`
    if (seen.has(key)) continue
    seen.add(key)
    refs.push({ slug: m[1], file: m[2] })
  }
  return refs
}

/** Rewrite every source image reference to its absolute public URL. */
export function rewriteBlogImagePaths(body: string): string {
  return body.replace(ASSET_REF_RE, (_full, slug: string, file: string) => publicImageUrl(slug, file))
}

/** Every image a post references must exist under public/blog/, or the export fails loudly. */
export async function assertImagesExist(body: string, publicDir: string, postSlug: string): Promise<void> {
  const missing: string[] = []
  for (const { slug, file } of listReferencedImages(body)) {
    const abs = path.join(publicDir, slug, file)
    const ok = await fs
      .stat(abs)
      .then((s) => s.isFile())
      .catch(() => false)
    if (!ok) missing.push(path.relative(WEBSITE_ROOT, abs))
  }
  if (missing.length) {
    throw new Error(
      `generate-blog-md: ${postSlug} references image(s) missing from public/blog/: ${missing.join(", ")}`,
    )
  }
}

export async function exportBlogMarkdown(
  contentDir = BLOG_CONTENT_DIR,
  distDir = BLOG_DIST,
  publicDir = BLOG_PUBLIC_DIR,
): Promise<number> {
  const posts = (await readBlogPosts(contentDir)).filter((post) => !post.draft)
  if (posts.length === 0) {
    throw new Error(`no publishable blog posts found in ${contentDir} — refusing to export an empty blog`)
  }

  const distOk = await fs
    .stat(distDir)
    .then((s) => s.isDirectory())
    .catch(() => false)
  if (!distOk) throw new Error(`generate-blog-md: ${distDir} missing — run \`next build\` first`)

  await Promise.all(
    posts.map(async (post) => {
      if (post.slug === "index") {
        throw new Error(`blog post slug "index" collides with the generated blog/index.md`)
      }
      const sourcePath = path.join(contentDir, post.sourceFile)
      const raw = await fs.readFile(sourcePath, "utf8")
      await assertImagesExist(raw, publicDir, post.slug)
      await fs.writeFile(path.join(distDir, `${post.slug}.md`), rewriteBlogImagePaths(raw), "utf8")
    }),
  )

  const indexLines = posts.map(formatBlogPostLinkLine)
  await fs.writeFile(
    path.join(distDir, "index.md"),
    [
      "# iii blog",
      "",
      "Architecture posts and examples for coding agents. Read as markdown.",
      "",
      `- [Blog home (HTML)](${SITE_ORIGIN}/blog/)`,
      "",
      ...indexLines,
      "",
    ].join("\n"),
    "utf8",
  )

  return posts.length
}

/** Markdown bullet list for llms.txt / AGENTS.md — links to agent-readable .md URLs. */
export async function buildBlogLinksSection(): Promise<string> {
  const posts = (await readBlogPosts()).filter((post) => !post.draft)
  if (posts.length === 0) return ""

  return [
    "## Blog (knowledge base for coding agents)",
    "",
    "Long-form architecture posts, harness patterns, and worked examples. Fetch as markdown:",
    "",
    `- [Blog index](${SITE_ORIGIN}/blog/index.md) — all posts`,
    ...posts.map(formatBlogPostLinkLine),
    "",
  ].join("\n")
}

async function main() {
  const count = await exportBlogMarkdown()
  console.log(`exported ${count} blog post(s) + index.md to ${path.relative(WEBSITE_ROOT, BLOG_DIST)}`)
}

const isMain = import.meta.url === pathToFileURL(path.resolve(process.argv[1] ?? "")).href
if (isMain) {
  main().catch((err) => {
    console.error(err)
    process.exitCode = 1
  })
}
