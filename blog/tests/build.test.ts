import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { test } from 'node:test'

const DIST = path.resolve(import.meta.dirname, '..', 'dist')

async function read(rel: string): Promise<string> {
  return readFile(path.join(DIST, rel), 'utf8')
}

test('blog index emits at /blog/ and links to sample post', async () => {
  const html = await read('index.html')
  assert.match(html, /href="\/blog\/hello-world\/"/, 'index should link to /blog/hello-world/')
  assert.match(html, /Hello, world/i, 'index should render the sample post title')
})

test('sample post emits at /blog/hello-world/index.html', async () => {
  const html = await read('hello-world/index.html')
  assert.match(html, /Hello, world/i)
  assert.match(html, /<article/i, 'post page should render an <article>')
})

test('rss feed exists and references the canonical post URL', async () => {
  const xml = await read('rss.xml')
  assert.match(xml, /<rss/i)
  assert.match(xml, /https:\/\/iii\.dev\/blog\/hello-world\//, 'rss should use absolute /blog/ URLs')
})

// Links to the parent iii.dev site that are allowed to escape the /blog/
// base path. Everything else must be /blog/-scoped or the Astro base config
// has regressed.
const PARENT_SITE_PATHS = new Set(['/', '/docs', '/favicon.svg'])

test('all internal links and assets are scoped under /blog/ (or the parent site allowlist)', async () => {
  const html = await read('index.html')
  const matches = [...html.matchAll(/(?:href|src)="(\/[^"]*)"/g)].map((m) => m[1])
  assert.ok(matches.length > 0, 'expected at least one internal link or asset')
  for (const url of matches) {
    if (PARENT_SITE_PATHS.has(url)) continue
    assert.ok(
      url.startsWith('/blog/') || url === '/blog' || url.startsWith('/blog?') || url.startsWith('/blog#'),
      `internal URL ${url} should be scoped under /blog/ (Astro base path) or added to PARENT_SITE_PATHS`,
    )
  }
})
