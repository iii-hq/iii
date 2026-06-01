import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { readBlogPosts } from './blog-posts'

async function makeFixture(files: Record<string, string>): Promise<string> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'iii-blog-posts-'))
  for (const [name, contents] of Object.entries(files)) {
    await fs.writeFile(path.join(dir, name), contents, 'utf8')
  }
  return dir
}

test('readBlogPosts parses frontmatter from a real post', async () => {
  const dir = await makeFixture({
    'first.md': `---
title: 'First'
description: 'desc'
pubDate: 2026-05-06
---

body
`,
  })
  const posts = await readBlogPosts(dir)
  assert.equal(posts.length, 1)
  assert.equal(posts[0].slug, 'first')
  assert.equal(posts[0].title, 'First')
  assert.equal(posts[0].draft, false)
  assert.equal(posts[0].pubDate.toISOString().slice(0, 10), '2026-05-06')
})

test('readBlogPosts honors draft frontmatter', async () => {
  const dir = await makeFixture({
    'wip.md': `---
title: 'WIP'
description: 'd'
pubDate: 2026-05-01
draft: true
---
`,
  })
  const posts = await readBlogPosts(dir)
  assert.equal(posts[0].draft, true)
})

test('readBlogPosts sorts newest first and ignores non-markdown files', async () => {
  const dir = await makeFixture({
    'a.md': `---
title: 'A'
description: 'd'
pubDate: 2026-01-01
---
`,
    'b.md': `---
title: 'B'
description: 'd'
pubDate: 2026-06-01
---
`,
    'README.txt': 'not a post',
  })
  const posts = await readBlogPosts(dir)
  assert.deepEqual(
    posts.map((p) => p.slug),
    ['b', 'a'],
  )
})

test('readBlogPosts returns empty array when dir is missing', async () => {
  const posts = await readBlogPosts('/nonexistent/path/iii-blog-posts')
  assert.deepEqual(posts, [])
})

test('readBlogPosts skips posts without a valid pubDate', async () => {
  const dir = await makeFixture({
    'broken.md': `---
title: 'Broken'
description: 'd'
---
`,
  })
  const posts = await readBlogPosts(dir)
  assert.deepEqual(posts, [])
})

test('readBlogPosts reads the real blog content directory and finds hello-world', async () => {
  const posts = await readBlogPosts()
  const hello = posts.find((p) => p.slug === 'hello-world')
  assert.ok(hello, 'expected the seeded hello-world post to be found')
  assert.equal(hello?.draft, false)
})
