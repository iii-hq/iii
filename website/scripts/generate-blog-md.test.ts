import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { exportBlogMarkdown, formatBlogPostLinkLine, rewriteBlogImagePaths } from './generate-blog-md'
import { SITE_ORIGIN } from './routes'

async function setupTempBlogDirs(): Promise<{ contentDir: string; distDir: string }> {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'iii-blog-md-'))
  const contentDir = path.join(root, 'content')
  const distDir = path.join(root, 'dist-blog')
  await fs.mkdir(contentDir, { recursive: true })
  await fs.mkdir(distDir, { recursive: true })
  return { contentDir, distDir }
}

async function writeBuiltPostHtml(distDir: string, slug: string, assetPaths: string[]): Promise<void> {
  await fs.mkdir(path.join(distDir, slug), { recursive: true })
  const imgs = assetPaths.map((p) => `<img src="${p}">`).join('')
  await fs.writeFile(path.join(distDir, slug, 'index.html'), `<html><body>${imgs}</body></html>`, 'utf8')
}

test('rewriteBlogImagePaths maps source assets to built asset URLs', () => {
  const body = '![banner](../../assets/blog/demo/banner.png)'
  const out = rewriteBlogImagePaths(body, ['/_astro/banner.DU5X-Ggo_1kFy9f.webp'])
  assert.equal(out, `![banner](${SITE_ORIGIN}/_astro/banner.DU5X-Ggo_1kFy9f.webp)`)
})

test('rewriteBlogImagePaths leaves unknown images untouched', () => {
  const body = '![banner](../../assets/blog/demo/banner.png)'
  assert.equal(rewriteBlogImagePaths(body, []), body)
})

test('formatBlogPostLinkLine escapes brackets in title', () => {
  const line = formatBlogPostLinkLine({
    slug: 'demo',
    sourceFile: 'demo.md',
    title: 'Loop [engineering]',
    description: 'desc',
    pubDate: new Date('2026-06-01'),
    draft: false,
  })
  assert.equal(line, `- [Loop \\[engineering\\]](${SITE_ORIGIN}/blog/demo.md) — desc`)
})

test('exportBlogMarkdown writes slug.md and index.md with asset URLs from the built page', async () => {
  const { contentDir, distDir } = await setupTempBlogDirs()
  await fs.writeFile(
    path.join(contentDir, 'demo.md'),
    `---
title: 'Demo'
description: 'A demo post'
pubDate: 2026-06-01
---

![banner](../../assets/blog/demo/banner.png)
`,
    'utf8',
  )
  await writeBuiltPostHtml(distDir, 'demo', ['/_astro/banner.abc123.webp'])

  const count = await exportBlogMarkdown(contentDir, distDir)
  assert.equal(count, 1)

  const exported = await fs.readFile(path.join(distDir, 'demo.md'), 'utf8')
  assert.match(exported, /title: 'Demo'/)
  assert.ok(exported.includes(`${SITE_ORIGIN}/_astro/banner.abc123.webp`))

  const index = await fs.readFile(path.join(distDir, 'index.md'), 'utf8')
  assert.ok(index.includes(`${SITE_ORIGIN}/blog/demo.md`))
})

test('exportBlogMarkdown resolves same-basename images per post from each built page', async () => {
  const { contentDir, distDir } = await setupTempBlogDirs()
  const posts = [
    { slug: 'post-a', hash: 'aaa111' },
    { slug: 'post-b', hash: 'bbb222' },
  ]
  for (const { slug, hash } of posts) {
    await fs.writeFile(
      path.join(contentDir, `${slug}.md`),
      `---
title: '${slug}'
description: 'd'
pubDate: 2026-06-01
---

![banner](../../assets/blog/${slug}/banner.png)
`,
      'utf8',
    )
    await writeBuiltPostHtml(distDir, slug, [`/_astro/banner.${hash}.webp`])
  }

  await exportBlogMarkdown(contentDir, distDir)

  const exportedA = await fs.readFile(path.join(distDir, 'post-a.md'), 'utf8')
  const exportedB = await fs.readFile(path.join(distDir, 'post-b.md'), 'utf8')
  assert.match(exportedA, /banner\.aaa111\.webp/)
  assert.match(exportedB, /banner\.bbb222\.webp/)
})

test('exportBlogMarkdown throws when no publishable posts exist', async () => {
  const { contentDir, distDir } = await setupTempBlogDirs()

  await assert.rejects(() => exportBlogMarkdown(contentDir, distDir), /no publishable blog posts/)
})

test('exportBlogMarkdown throws when a post has no built page yet', async () => {
  const { contentDir, distDir } = await setupTempBlogDirs()
  await fs.writeFile(
    path.join(contentDir, 'unbuilt.md'),
    `---
title: 'Unbuilt'
description: 'd'
pubDate: 2026-06-01
---
`,
    'utf8',
  )

  await assert.rejects(() => exportBlogMarkdown(contentDir, distDir), /run `astro build` first/)
})

test('exportBlogMarkdown reads .mdx sources and writes slug.md output', async () => {
  const { contentDir, distDir } = await setupTempBlogDirs()
  await fs.writeFile(
    path.join(contentDir, 'mdx-post.mdx'),
    `---
title: 'MDX post'
description: 'From mdx source'
pubDate: 2026-06-02
---

MDX body
`,
    'utf8',
  )
  await writeBuiltPostHtml(distDir, 'mdx-post', [])

  const count = await exportBlogMarkdown(contentDir, distDir)
  assert.equal(count, 1)

  const exported = await fs.readFile(path.join(distDir, 'mdx-post.md'), 'utf8')
  assert.match(exported, /title: 'MDX post'/)
  assert.match(exported, /MDX body/)

  const index = await fs.readFile(path.join(distDir, 'index.md'), 'utf8')
  assert.ok(index.includes(`${SITE_ORIGIN}/blog/mdx-post.md`))
})
