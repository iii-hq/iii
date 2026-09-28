import assert from "node:assert/strict"
import fs from "node:fs/promises"
import os from "node:os"
import path from "node:path"
import test from "node:test"
import {
  exportBlogMarkdown,
  formatBlogPostLinkLine,
  listReferencedImages,
  rewriteBlogImagePaths,
} from "./generate-blog-md"
import { SITE_ORIGIN } from "./routes"

async function setupTempBlogDirs(): Promise<{ contentDir: string; distDir: string; publicDir: string }> {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "iii-blog-md-"))
  const contentDir = path.join(root, "content")
  const distDir = path.join(root, "dist-blog")
  const publicDir = path.join(root, "public-blog")
  await fs.mkdir(contentDir, { recursive: true })
  await fs.mkdir(distDir, { recursive: true })
  await fs.mkdir(publicDir, { recursive: true })
  return { contentDir, distDir, publicDir }
}

async function writePublicImage(publicDir: string, slug: string, file: string): Promise<void> {
  await fs.mkdir(path.join(publicDir, slug), { recursive: true })
  await fs.writeFile(path.join(publicDir, slug, file), "png", "utf8")
}

test("rewriteBlogImagePaths maps source refs to the public/blog URL", () => {
  const body = "![banner](../../assets/blog/demo/banner.png)"
  assert.equal(rewriteBlogImagePaths(body), `![banner](${SITE_ORIGIN}/blog/demo/banner.png)`)
})

test("rewriteBlogImagePaths rewrites frontmatter ogImage and keeps a markdown title", () => {
  const body = `---
ogImage: ../../assets/blog/demo/banner.png
---

![shot](../../assets/blog/demo/shot.png "A caption")
`
  const out = rewriteBlogImagePaths(body)
  assert.match(out, new RegExp(`ogImage: ${SITE_ORIGIN}/blog/demo/banner.png\n`))
  assert.ok(out.includes(`![shot](${SITE_ORIGIN}/blog/demo/shot.png "A caption")`))
  assert.ok(!out.includes("../../assets/blog/"))
})

test("rewriteBlogImagePaths leaves unrelated paths untouched", () => {
  const body = "![ext](https://example.com/x.png) and [doc](../docs/x.md)"
  assert.equal(rewriteBlogImagePaths(body), body)
})

test("listReferencedImages dedupes and keeps the slug from the reference", () => {
  const body = [
    "![a](../../assets/blog/post-a/banner.png)",
    "![a again](../../assets/blog/post-a/banner.png)",
    "![b](../../assets/blog/post-b/diagram.png)",
  ].join("\n")
  assert.deepEqual(listReferencedImages(body), [
    { slug: "post-a", file: "banner.png" },
    { slug: "post-b", file: "diagram.png" },
  ])
})

test("formatBlogPostLinkLine escapes brackets in title", () => {
  const line = formatBlogPostLinkLine({
    slug: "demo",
    sourceFile: "demo.md",
    title: "Loop [engineering]",
    description: "desc",
    pubDate: new Date("2026-06-01"),
    draft: false,
  })
  assert.equal(line, `- [Loop \\[engineering\\]](${SITE_ORIGIN}/blog/demo.md) — desc`)
})

test("exportBlogMarkdown writes slug.md and index.md with public image URLs", async () => {
  const { contentDir, distDir, publicDir } = await setupTempBlogDirs()
  await fs.writeFile(
    path.join(contentDir, "demo.md"),
    `---
title: 'Demo'
description: 'A demo post'
pubDate: 2026-06-01
ogImage: ../../assets/blog/demo/banner.png
---

![banner](../../assets/blog/demo/banner.png)
`,
    "utf8",
  )
  await writePublicImage(publicDir, "demo", "banner.png")

  const count = await exportBlogMarkdown(contentDir, distDir, publicDir)
  assert.equal(count, 1)

  const exported = await fs.readFile(path.join(distDir, "demo.md"), "utf8")
  assert.match(exported, /title: 'Demo'/)
  assert.ok(exported.includes(`![banner](${SITE_ORIGIN}/blog/demo/banner.png)`))
  assert.ok(exported.includes(`ogImage: ${SITE_ORIGIN}/blog/demo/banner.png`))
  assert.ok(!exported.includes("../../assets/"))

  const index = await fs.readFile(path.join(distDir, "index.md"), "utf8")
  assert.ok(index.includes("# iii blog"))
  assert.ok(index.includes(`${SITE_ORIGIN}/blog/`))
  assert.ok(index.includes(`- [Demo](${SITE_ORIGIN}/blog/demo.md) — A demo post`))
})

test("exportBlogMarkdown fails loudly when a referenced image is missing from public/", async () => {
  const { contentDir, distDir, publicDir } = await setupTempBlogDirs()
  await fs.writeFile(
    path.join(contentDir, "demo.md"),
    `---
title: 'Demo'
description: 'd'
pubDate: 2026-06-01
---

![banner](../../assets/blog/demo/missing.png)
`,
    "utf8",
  )

  await assert.rejects(
    () => exportBlogMarkdown(contentDir, distDir, publicDir),
    /demo references image\(s\) missing from public\/blog\/: .*demo\/missing\.png/,
  )
})

test("exportBlogMarkdown skips drafts", async () => {
  const { contentDir, distDir, publicDir } = await setupTempBlogDirs()
  for (const [slug, draft] of [
    ["live", "false"],
    ["wip", "true"],
  ]) {
    await fs.writeFile(
      path.join(contentDir, `${slug}.md`),
      `---
title: '${slug}'
description: 'd'
pubDate: 2026-06-01
draft: ${draft}
---
`,
      "utf8",
    )
  }

  assert.equal(await exportBlogMarkdown(contentDir, distDir, publicDir), 1)
  assert.ok(await fs.stat(path.join(distDir, "live.md")))
  await assert.rejects(() => fs.stat(path.join(distDir, "wip.md")))
  const index = await fs.readFile(path.join(distDir, "index.md"), "utf8")
  assert.ok(!index.includes("wip.md"))
})

test("exportBlogMarkdown throws when no publishable posts exist", async () => {
  const { contentDir, distDir, publicDir } = await setupTempBlogDirs()

  await assert.rejects(() => exportBlogMarkdown(contentDir, distDir, publicDir), /no publishable blog posts/)
})

test("exportBlogMarkdown throws when dist/blog does not exist yet", async () => {
  const { contentDir, publicDir } = await setupTempBlogDirs()
  await fs.writeFile(
    path.join(contentDir, "unbuilt.md"),
    `---
title: 'Unbuilt'
description: 'd'
pubDate: 2026-06-01
---
`,
    "utf8",
  )

  await assert.rejects(
    () => exportBlogMarkdown(contentDir, path.join(os.tmpdir(), "iii-blog-md-nonexistent-dist"), publicDir),
    /run `next build` first/,
  )
})

test("exportBlogMarkdown reads .mdx sources and writes slug.md output", async () => {
  const { contentDir, distDir, publicDir } = await setupTempBlogDirs()
  await fs.writeFile(
    path.join(contentDir, "mdx-post.mdx"),
    `---
title: 'MDX post'
description: 'From mdx source'
pubDate: 2026-06-02
---

MDX body
`,
    "utf8",
  )

  const count = await exportBlogMarkdown(contentDir, distDir, publicDir)
  assert.equal(count, 1)

  const exported = await fs.readFile(path.join(distDir, "mdx-post.md"), "utf8")
  assert.match(exported, /title: 'MDX post'/)
  assert.match(exported, /MDX body/)

  const index = await fs.readFile(path.join(distDir, "index.md"), "utf8")
  assert.ok(index.includes(`${SITE_ORIGIN}/blog/mdx-post.md`))
})

test("every real post's images exist under public/blog/", async () => {
  const contentDir = path.resolve(import.meta.dirname, "../src/content/blog")
  const publicDir = path.resolve(import.meta.dirname, "../public/blog")
  const files = (await fs.readdir(contentDir)).filter((f) => /\.mdx?$/.test(f))
  assert.ok(files.length > 0)
  for (const file of files) {
    const body = await fs.readFile(path.join(contentDir, file), "utf8")
    for (const { slug, file: img } of listReferencedImages(body)) {
      const stat = await fs.stat(path.join(publicDir, slug, img)).catch(() => null)
      assert.ok(stat?.isFile(), `${file}: public/blog/${slug}/${img} missing`)
    }
  }
})
