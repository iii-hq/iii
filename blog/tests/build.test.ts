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
  assert.match(html, /href="\/blog\/add-a-worker\/"/, 'index should link to /blog/add-a-worker/')
  assert.match(html, /Add a worker/i, 'index should render the sample post title')
})

test('sample post emits at /blog/add-a-worker/index.html', async () => {
  const html = await read('add-a-worker/index.html')
  assert.match(html, /Add a worker/i)
  assert.match(html, /<article/i, 'post page should render an <article>')
})

test('rss feed exists and references the canonical post URL', async () => {
  const xml = await read('rss.xml')
  assert.match(xml, /<rss/i)
  assert.match(xml, /https:\/\/iii\.dev\/blog\/add-a-worker\//, 'rss should use absolute /blog/ URLs')
})

// Links to the parent iii.dev site that are allowed to escape the /blog/
// base path. Everything else must be /blog/-scoped or the Astro base config
// has regressed.
const PARENT_SITE_PATHS = new Set([
  '/',
  '/docs',
  '/docs/quickstart',
  '/favicon.svg',
  '/fonts/ChivoMono-VariableFont_wght.ttf',
  '/fonts/ChivoMono-Italic-VariableFont_wght.ttf',
])

// Analytics + consent — must mirror website/index.html so the localStorage
// 'iii_cookie_consent' key is shared across iii.dev and /blog. If any of
// these regress, the blog will either lose tracking or the marketing site's
// consent state will silently desync.
async function assertAnalyticsAndConsent(html: string, where: string) {
  assert.match(html, /GTM-N8DCTFB8/, `${where}: GTM container ID missing`)
  assert.match(html, /googletagmanager\.com\/ns\.html\?id=GTM-N8DCTFB8/, `${where}: GTM <noscript> iframe missing`)
  assert.match(html, /iiiLoadCommonRoomSignals/, `${where}: Common Room loader missing`)
  assert.match(html, /iiiNotifyCommonRoomEmail/, `${where}: Common Room email hook missing`)
  assert.match(html, /cdn\.cr-relay\.com\/v1\/site\/da18833a-8f00-4ad0-9833-6608b59a713a\/signals\.js/, `${where}: Common Room signals script URL missing`)
  assert.match(html, /id="cookie-consent-banner"/, `${where}: cookie consent banner DOM missing`)
  assert.match(html, /'iii_cookie_consent'/, `${where}: shared consent storage key missing`)
}

test('blog index includes GTM, Common Room loader, and cookie consent banner', async () => {
  const html = await read('index.html')
  await assertAnalyticsAndConsent(html, 'index.html')
})

test('blog post page includes GTM, Common Room loader, and cookie consent banner', async () => {
  const html = await read('add-a-worker/index.html')
  await assertAnalyticsAndConsent(html, 'add-a-worker/index.html')
})

// Theme + branding parity with iii.dev: the same 'iii_theme' localStorage key
// must drive both sites and the iii logo SVG must render in the header.
test('blog pages share the iii_theme key and apply dark-mode tokens before paint', async () => {
  const html = await read('index.html')
  assert.match(html, /'iii_theme'/, 'theme storage key missing — would desync from website')
  assert.match(html, /__iiiThemeInit/, 'theme init flag missing')
  assert.match(html, /dataset\.theme\s*=\s*'dark'/, 'dark theme application missing')
})

test('blog header renders the iii six-rect logo', async () => {
  const html = await read('index.html')
  assert.match(html, /viewBox="0 0 1075\.74 1075\.74"/, 'iii logo SVG viewBox missing')
  assert.match(html, /class="bar"/, 'logo bar rects missing')
  assert.match(html, /class="accent"/, 'logo accent rects missing')
})

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
