// Post-build contract for the whole iii.dev dist/ tree. Run via `pnpm test:dist`
// after `pnpm build`. Guards the three surfaces one build now emits — the
// marketing pages (Astro), the blog (Astro content collection), and the
// roadmap (roadmap/build.mjs) — plus the generated SEO artifacts.
import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const DIST = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'dist')

async function read(rel: string): Promise<string> {
  return readFile(path.join(DIST, rel), 'utf8')
}

test('dist exists (run `pnpm build` before `pnpm test:dist`)', () => {
  assert.ok(existsSync(DIST), 'dist/ missing — run `pnpm build` first')
})

test('every surface emits its entry page and static assets', () => {
  const required = [
    'index.html',
    'manifesto.html',
    'privacy-policy.html',
    'blog/index.html',
    'blog/rss.xml',
    'roadmap/index.html',
    'roadmap/index.json',
    'sitemap.xml',
    'llms.txt',
    'AGENTS.md',
    'robots.txt',
    'favicon.svg',
    'og-image.png',
    'posthog-consent.js',
    'fonts/ChivoMono-VariableFont_wght.ttf',
    'fonts/ChivoMono-Italic-VariableFont_wght.ttf',
  ]
  for (const rel of required) {
    assert.ok(existsSync(path.join(DIST, rel)), `missing dist/${rel}`)
  }
})

test('landing page keeps its interactive markup and canonical', async () => {
  const html = await read('index.html')
  assert.match(html, /id="hero-viz"/, 'hero viz mount missing')
  assert.match(html, /id="cs-scroll"/, 'console side-scroll section missing')
  assert.match(html, /<link rel="canonical" href="https:\/\/iii\.dev\/"/, 'canonical must stay https://iii.dev/')
  assert.match(html, /application\/ld\+json/, 'JSON-LD blocks missing')
  assert.match(html, /iii:mailmodo-form-url/, 'Mailmodo form-url meta missing')
})

test('manifesto and privacy keep their extensionless canonicals', async () => {
  assert.match(await read('manifesto.html'), /<link rel="canonical" href="https:\/\/iii\.dev\/manifesto"/)
  assert.match(await read('privacy-policy.html'), /<link rel="canonical" href="https:\/\/iii\.dev\/privacy-policy"/)
})

test('blog index emits at /blog/ and links to sample post', async () => {
  const html = await read('blog/index.html')
  assert.match(html, /href="\/blog\/add-a-worker\/"/, 'index should link to /blog/add-a-worker/')
  assert.match(html, /Add a worker/i, 'index should render the sample post title')
  assert.match(html, /id="theme-btn"/, 'index should include theme toggle (parity with iii.dev)')
})

test('sample post emits at /blog/add-a-worker/index.html with article JSON-LD', async () => {
  const html = await read('blog/add-a-worker/index.html')
  assert.match(html, /Add a worker/i)
  assert.match(html, /<article/i, 'post page should render an <article>')
  assert.match(html, /"@type":"BlogPosting"/, 'post page should carry BlogPosting JSON-LD')
  assert.match(html, /property="og:type" content="article"/, 'posts should use og:type article')
})

test('rss feed exists and references the canonical post URL', async () => {
  const xml = await read('blog/rss.xml')
  assert.match(xml, /<rss/i)
  assert.match(xml, /<link>https:\/\/iii\.dev\/blog\/<\/link>/, 'channel link should be blog home')
  assert.match(xml, /atom:link[^>]*href="https:\/\/iii\.dev\/blog\/rss\.xml"/, 'feed should advertise atom:self URL')
  assert.match(xml, /https:\/\/iii\.dev\/blog\/add-a-worker\//, 'rss should use absolute /blog/ URLs')
})

// Analytics + consent — the same DOM and localStorage contract on every
// surface, so the visitor's decision travels across iii.dev, /blog, and the
// marketing pages. Mirrors the old blog/tests/build.test.ts contract.
async function assertAnalyticsAndConsent(html: string, where: string) {
  assert.match(html, /GTM-N8DCTFB8/, `${where}: GTM container ID missing`)
  assert.match(html, /googletagmanager\.com\/ns\.html\?id=GTM-N8DCTFB8/, `${where}: GTM <noscript> iframe missing`)
  assert.match(html, /iiiLoadCommonRoomSignals/, `${where}: Common Room loader missing`)
  assert.match(html, /iiiNotifyCommonRoomEmail/, `${where}: Common Room email hook missing`)
  assert.match(html, /id="cookie-consent-banner"/, `${where}: cookie consent banner DOM missing`)
  assert.match(html, /'iii_cookie_consent'/, `${where}: shared consent storage key missing`)
}

for (const page of [
  'index.html',
  'manifesto.html',
  'privacy-policy.html',
  'blog/index.html',
  'blog/add-a-worker/index.html',
]) {
  test(`${page} includes GTM, Common Room loader, and cookie consent banner`, async () => {
    await assertAnalyticsAndConsent(await read(page), page)
  })
}

test('blog pages share the iii_theme key and apply dark-mode tokens before paint', async () => {
  const html = await read('blog/index.html')
  assert.match(html, /'iii_theme'/, 'theme storage key missing — would desync from the landing page')
  assert.match(html, /__iiiThemeInit/, 'theme init flag missing')
  assert.match(html, /dataset\.theme\s*=\s*'dark'/, 'dark theme application missing')
})

test('blog header renders the iii six-rect logo', async () => {
  const html = await read('blog/index.html')
  assert.match(html, /viewBox="0 0 1075\.74 1075\.74"/, 'iii logo SVG viewBox missing')
  assert.match(html, /class="bar"/, 'logo bar rects missing')
  assert.match(html, /class="accent"/, 'logo accent rects missing')
})

test('sitemap covers all three surfaces with the load-bearing URL shapes', async () => {
  const xml = await read('sitemap.xml')
  assert.match(xml, /<loc>https:\/\/iii\.dev\/<\/loc>/)
  assert.match(
    xml,
    /<loc>https:\/\/iii\.dev\/manifesto<\/loc>/,
    'marketing pages stay extensionless, no trailing slash',
  )
  assert.match(xml, /<loc>https:\/\/iii\.dev\/blog\/add-a-worker\/<\/loc>/, 'blog posts keep trailing-slash URLs')
  assert.match(xml, /<loc>https:\/\/iii\.dev\/roadmap\/<\/loc>/, 'roadmap gallery listed')
})

test('llms.txt and AGENTS.md are generated from the built homepage', async () => {
  const llms = await read('llms.txt')
  assert.ok(llms.startsWith('# iii\n'), 'llms.txt must open with the project H1')
  assert.match(llms, /Homepage copy \(extracted/, 'homepage extract missing from llms.txt')
  const agents = await read('AGENTS.md')
  assert.ok(agents.startsWith('# iii for AI Agents'), 'AGENTS.md heading drifted')
})

test('roadmap gallery and deck pages emit under /roadmap/', async () => {
  const feed = JSON.parse(await read('roadmap/index.json')) as { specs: { slug: string; url: string }[] }
  assert.ok(feed.specs.length > 0, 'roadmap feed should list published specs')
  for (const spec of feed.specs) {
    assert.match(spec.url, /^\/roadmap\//, 'feed URLs must stay /roadmap/-prefixed')
    assert.ok(existsSync(path.join(DIST, 'roadmap', spec.slug, 'index.html')), `missing roadmap page for ${spec.slug}`)
  }
})

test('the shared site header is on every non-deck surface, roadmap gallery included', async () => {
  for (const page of ['index.html', 'manifesto.html', 'privacy-policy.html', 'blog/index.html', 'roadmap/index.html']) {
    const html = await read(page)
    assert.match(html, /<nav class="nav" id="nav">/, `${page}: shared SiteNav missing`)
    assert.match(html, /class="nav-link" href="\/roadmap\/"/, `${page}: roadmap link missing from header`)
    assert.doesNotMatch(html, /class="nav-link" href="#/, `${page}: scroll-only nav links should be gone`)
  }
})
