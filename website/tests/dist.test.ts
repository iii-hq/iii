// Post-build contract for the whole iii.dev dist/ tree. Run via `pnpm test:dist`
// after `pnpm build`. One Next.js static export emits every surface: the
// marketing pages (top-level *.html, served extensionless via the KVS route
// map), the blog and the roadmap (directory sites, <route>/index.html after
// scripts/finalize-dist.ts), plus the generated SEO artifacts (blog .md twins,
// llms.txt, AGENTS.md, sitemap.xml). The URL shapes asserted here are the
// site's canonical URLs; see infra/terraform/website/cloudfront_functions/redirects.js.
import assert from "node:assert/strict"
import { existsSync, readdirSync } from "node:fs"
import { readFile } from "node:fs/promises"
import path from "node:path"
import { test } from "node:test"
import { fileURLToPath } from "node:url"
import { readBlogPosts } from "../scripts/blog-posts"
import { desiredRoutes } from "../scripts/routes-kvs"

const DIST = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "dist")
const SAMPLE_POST = "add-a-worker"

async function read(rel: string): Promise<string> {
  return readFile(path.join(DIST, rel), "utf8")
}

const has = (rel: string) => existsSync(path.join(DIST, rel))

test("dist exists (run `pnpm build` before `pnpm test:dist`)", () => {
  assert.ok(existsSync(DIST), "dist/ missing — run `pnpm build` first")
})

test("every surface emits its entry page, generated artifacts and static assets", () => {
  const required = [
    "index.html",
    "manifesto.html",
    "privacy-policy.html",
    "404.html",
    "blog/index.html",
    `blog/${SAMPLE_POST}/index.html`,
    `blog/${SAMPLE_POST}.md`,
    "blog/index.md",
    "blog/rss.xml",
    "roadmap/index.html",
    "roadmap/index.json",
    "sitemap.xml",
    "llms.txt",
    "AGENTS.md",
    "robots.txt",
    "favicon.svg",
    "og-image.png",
    "posthog-consent.js",
    "console-demo/index.html",
  ]
  for (const rel of required) {
    assert.ok(has(rel), `missing dist/${rel}`)
  }
})

test("finalize-dist gave the blog and roadmap their directory shape", () => {
  assert.ok(!has("blog.html"), "blog.html should have moved to blog/index.html")
  assert.ok(!has("roadmap.html"), "roadmap.html should have moved to roadmap/index.html")
  for (const site of ["blog", "roadmap"]) {
    const stray = readdirSync(path.join(DIST, site)).filter((f) => f.endsWith(".html") && f !== "index.html")
    assert.deepEqual(stray, [], `${site}/ still has page files outside a directory: ${stray.join(", ")}`)
  }
})

test("the KVS route map covers exactly the extensionless marketing pages", () => {
  const topLevel = readdirSync(DIST, { withFileTypes: true })
    .filter((d) => d.isFile())
    .map((d) => d.name)
  assert.deepEqual(desiredRoutes(topLevel), [
    { Key: "/manifesto", Value: "/manifesto.html" },
    { Key: "/privacy-policy", Value: "/privacy-policy.html" },
  ])
})

test("console demo entry uses absolute asset paths that exist", async () => {
  const html = await read("console-demo/index.html")
  const refs = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((m) => m[1]).filter((u) => u.includes("assets/"))
  assert.ok(refs.length >= 2, "demo entry references no hashed assets")
  const assetsRoot = path.resolve(DIST, "console-demo/assets")
  for (const u of refs) {
    // Relative URLs break behind Vercel's cleanUrls: /console-demo/index.html
    // 308s to /console-demo (no trailing slash), so ./assets/* resolves to
    // the domain root and the bundle 404s on preview deploys.
    assert.ok(
      u.startsWith("/console-demo/assets/"),
      `demo asset URL must be absolute under /console-demo/assets/, got ${u}`,
    )
    // Resolve before touching the filesystem so an encoded or ../ path in a
    // tampered artifact can't satisfy the check with a file outside the
    // vendored directory.
    const assetPath = path.resolve(DIST, decodeURIComponent(new URL(u, "https://dist.invalid").pathname).slice(1))
    assert.ok(assetPath.startsWith(`${assetsRoot}${path.sep}`), `demo asset URL escapes the asset directory: ${u}`)
    assert.ok(existsSync(assetPath), `demo asset missing from dist: ${u}`)
  }
})

// --- landing page ---------------------------------------------------------

test("landing page keeps its section anchors, llms hooks and structured data", async () => {
  const html = await read("index.html")
  for (const id of ["hero", "console-live", "harness", "experience", "hello", "workers", "nutshell", "footer"]) {
    assert.match(html, new RegExp(`id="${id}"`), `#${id} section missing`)
  }
  assert.match(html, /data-llms="hero"/, "hero copy hook missing (scripts/generate-llms-agents.ts)")
  assert.match(html, /data-llms="intro"/, "section intro hook missing (scripts/generate-llms-agents.ts)")
  assert.match(html, /class="nw-cta-row/, "#workers CTA row missing (scripts/generate-llms-agents.ts)")
  assert.match(html, /<script type="application\/ld\+json">/, "JSON-LD block missing")
  assert.match(html, /"@type":"Organization"/, "Organization JSON-LD missing")
  assert.match(html, /"@type":"WebSite"/, "WebSite JSON-LD missing")
  assert.match(html, /"@type":"SoftwareApplication"/, "SoftwareApplication JSON-LD missing")
  assert.match(html, /<title>iii — Three primitives\. Zero integration cost\.<\/title>/)
  assert.match(html, /property="og:type" content="website"/)
})

// --- canonical URL shapes --------------------------------------------------

test("marketing pages keep their extensionless canonicals", async () => {
  assert.match(await read("manifesto.html"), /<link rel="canonical" href="https:\/\/iii\.dev\/manifesto"\/>/)
  assert.match(await read("privacy-policy.html"), /<link rel="canonical" href="https:\/\/iii\.dev\/privacy-policy"\/>/)
})

test("blog and roadmap pages keep their trailing-slash canonicals", async () => {
  assert.match(await read("blog/index.html"), /<link rel="canonical" href="https:\/\/iii\.dev\/blog\/"\/>/)
  assert.match(
    await read(`blog/${SAMPLE_POST}/index.html`),
    new RegExp(`<link rel="canonical" href="https://iii\\.dev/blog/${SAMPLE_POST}/"/>`),
  )
  assert.match(await read("roadmap/index.html"), /<link rel="canonical" href="https:\/\/iii\.dev\/roadmap\/"\/>/)
})

test("404.html is a real not-found document", async () => {
  const html = await read("404.html")
  assert.match(html, /<html lang="en"/)
  assert.match(html, /Page not found/)
})

// --- blog -------------------------------------------------------------------

test("blog index emits at /blog/ and links to the sample post", async () => {
  const html = await read("blog/index.html")
  assert.match(html, new RegExp(`href="/blog/${SAMPLE_POST}/?"`), `index should link to /blog/${SAMPLE_POST}/`)
  assert.match(html, /Add a worker/i, "index should render the sample post title")
  assert.match(html, /application\/rss\+xml/, "index should advertise the RSS feed")
})

test("sample post emits at /blog/<slug>/index.html with article JSON-LD and its images", async () => {
  const html = await read(`blog/${SAMPLE_POST}/index.html`)
  assert.match(html, /Add a worker/i)
  assert.match(html, /<article/i, "post page should render an <article>")
  assert.match(html, /"@type":"BlogPosting"/, "post page should carry BlogPosting JSON-LD")
  assert.match(html, new RegExp(`"mainEntityOfPage":"https://iii\\.dev/blog/${SAMPLE_POST}/"`))
  assert.match(html, /property="og:type" content="article"/, "posts should use og:type article")
  assert.match(html, /property="article:published_time"/, "posts should carry article:published_time")
  // Every image the page references under /blog/<slug>/ is a real file in dist (copied from public/blog/).
  const refs = new Set(
    [...html.matchAll(/(?:src|href|content)="(\/blog\/[^"]+\.(?:png|jpe?g|webp|gif|svg))"/g)].map((m) => m[1]),
  )
  assert.ok(refs.size >= 1, "post should reference at least its banner image")
  for (const ref of refs) assert.ok(has(ref.slice(1)), `image referenced by the post is missing from dist: ${ref}`)
})

test("blog .md twins point images at https://iii.dev/blog/<slug>/ and list every published post", async () => {
  const post = await read(`blog/${SAMPLE_POST}.md`)
  assert.match(post, /^---\n/, "post .md keeps its frontmatter")
  assert.match(post, new RegExp(`\\]\\(https://iii\\.dev/blog/${SAMPLE_POST}/banner\\.png\\)`))
  assert.doesNotMatch(post, /\.\.\/\.\.\/assets\//, "source-relative image paths must be rewritten")

  const index = await read("blog/index.md")
  assert.ok(index.startsWith("# iii blog\n"))
  assert.match(index, /\(https:\/\/iii\.dev\/blog\/\)/, "index.md links the HTML blog home with a trailing slash")
  const posts = (await readBlogPosts()).filter((p) => !p.draft)
  assert.ok(posts.length > 0)
  for (const p of posts) {
    assert.ok(has(`blog/${p.slug}.md`), `missing dist/blog/${p.slug}.md`)
    assert.ok(has(`blog/${p.slug}/index.html`), `missing dist/blog/${p.slug}/index.html`)
    assert.ok(index.includes(`https://iii.dev/blog/${p.slug}.md`), `index.md should list ${p.slug}`)
  }
})

test("rss feed exists and references the blog home and the sample post", async () => {
  const xml = await read("blog/rss.xml")
  assert.match(xml, /<rss/i)
  assert.match(xml, /<link>https:\/\/iii\.dev\/blog\/<\/link>/, "channel link should be blog home")
  assert.match(xml, /atom:link[^>]*href="https:\/\/iii\.dev\/blog\/rss\.xml"/, "feed should advertise atom:self URL")
  assert.match(
    xml,
    new RegExp(`<link>https://iii\\.dev/blog/${SAMPLE_POST}/?</link>`),
    "rss should use absolute /blog/ URLs",
  )
})

// --- analytics, consent, theme, logo ---------------------------------------

// Analytics + consent — the same DOM and localStorage contract on every
// surface, so the visitor's decision travels across iii.dev, /blog, and the
// marketing pages. Production builds load the trackers by default
// (src/lib/analytics.ts), gated on the shared consent key.
async function assertAnalyticsAndConsent(html: string, where: string) {
  assert.match(html, /GTM-N8DCTFB8/, `${where}: GTM container ID missing`)
  assert.match(html, /googletagmanager\.com\/ns\.html\?id=GTM-N8DCTFB8/, `${where}: GTM <noscript> iframe missing`)
  assert.match(html, /iiiLoadCommonRoomSignals/, `${where}: Common Room loader missing`)
  assert.match(html, /iiiNotifyCommonRoomEmail/, `${where}: Common Room email hook missing`)
  assert.match(html, /"iii_cookie_consent"/, `${where}: shared consent storage key missing`)
  assert.match(
    html,
    /<script src="\/posthog-consent\.js" async(=""|)><\/script>/,
    `${where}: posthog-consent.js loader missing`,
  )
}

const SHARED_PAGES = [
  "index.html",
  "manifesto.html",
  "privacy-policy.html",
  "blog/index.html",
  `blog/${SAMPLE_POST}/index.html`,
]

for (const page of SHARED_PAGES) {
  test(`${page} includes GTM, Common Room loader, and the cookie consent key`, async () => {
    await assertAnalyticsAndConsent(await read(page), page)
  })
}

test("every page shares the iii_theme key and applies the dark class before paint", async () => {
  for (const page of SHARED_PAGES) {
    const html = await read(page)
    assert.match(html, /"iii_theme"/, `${page}: theme storage key missing — would desync across pages`)
    assert.match(html, /classList\.toggle\("dark",/, `${page}: dark class application missing`)
    assert.match(html, /<html lang="en" class="[^"]*\bdark\b/, `${page}: dark-first <html> class missing`)
  }
})

test("the shared header and footer are on every surface, roadmap included", async () => {
  for (const page of [...SHARED_PAGES, "roadmap/index.html"]) {
    const html = await read(page)
    assert.match(html, /<header class="fixed/, `${page}: shared site header missing`)
    assert.match(html, /href="\/roadmap"/, `${page}: roadmap link missing from header`)
    assert.match(html, /<footer/, `${page}: shared site footer missing`)
    assert.match(html, /Pronounced “three eye”\./, `${page}: footer tagline missing`)
    assert.match(html, /viewBox="0 0 1075\.74 1075\.74"/, `${page}: iii logo SVG viewBox missing`)
  }
})

// --- SEO artifacts -----------------------------------------------------------

test("sitemap covers the indexable surfaces with the load-bearing URL shapes", async () => {
  const xml = await read("sitemap.xml")
  assert.match(xml, /<loc>https:\/\/iii\.dev\/<\/loc>/)
  assert.match(
    xml,
    /<loc>https:\/\/iii\.dev\/manifesto<\/loc>/,
    "marketing pages stay extensionless, no trailing slash",
  )
  assert.match(xml, /<loc>https:\/\/iii\.dev\/privacy-policy<\/loc>/)
  assert.match(xml, /<loc>https:\/\/iii\.dev\/blog\/<\/loc>/, "blog home keeps its trailing slash")
  assert.match(
    xml,
    new RegExp(`<loc>https://iii\\.dev/blog/${SAMPLE_POST}/</loc>`),
    "blog posts keep trailing-slash URLs",
  )
  assert.match(
    xml,
    new RegExp(`<loc>https://iii\\.dev/blog/${SAMPLE_POST}\\.md</loc>`),
    "blog posts list their .md twin",
  )
  assert.match(xml, /<loc>https:\/\/iii\.dev\/blog\/index\.md<\/loc>/)
  assert.match(xml, /<loc>https:\/\/iii\.dev\/llms\.txt<\/loc>/)
  assert.match(xml, /<loc>https:\/\/iii\.dev\/AGENTS\.md<\/loc>/)
  assert.doesNotMatch(xml, /\/roadmap/, "roadmap stays out of the sitemap (robots.txt disallows it)")
  assert.doesNotMatch(xml, /\.html</, "no .html URLs leak into the sitemap")
  for (const loc of xml.matchAll(/<loc>([^<]+)<\/loc>/g)) {
    assert.ok(loc[1].startsWith("https://iii.dev/"), `sitemap URL must be absolute: ${loc[1]}`)
  }
})

test("robots.txt keeps the roadmap out of every crawler block and points at the sitemap", async () => {
  const robots = await read("robots.txt")
  const blocks = robots.split(/\n(?=User-agent:)/).filter((b) => b.startsWith("User-agent:"))
  assert.ok(blocks.length > 1, "robots.txt should keep its per-crawler blocks")
  for (const block of blocks) {
    if (/^Disallow: \/$/m.test(block)) continue // blanket opt-out (training-only crawlers)
    assert.match(block, /^Disallow: \/roadmap$/m, `crawler block missing roadmap disallow:\n${block}`)
  }
  assert.match(robots, /^Sitemap: https:\/\/iii\.dev\/sitemap\.xml$/m)
})

test("llms.txt and AGENTS.md are generated from the built homepage and read as prose", async () => {
  const llms = await read("llms.txt")
  assert.ok(llms.startsWith("# iii\n"), "llms.txt must open with the project H1")
  assert.match(llms, /## Homepage copy \(extracted from iii\.dev HTML\)/, "homepage extract missing from llms.txt")
  assert.match(
    llms,
    /### Hero\n\*\*Stop integrating services\. Start adding them\.\*\*\n/,
    "hero headline should read as one sentence pair",
  )
  assert.match(llms, /### iii in a nutshell\n/)
  assert.match(llms, /^- Durable orchestration: /m, "nutshell cards should be bullets")
  assert.match(llms, /### Footer \/ links\n/)
  assert.match(llms, /https:\/\/iii\.dev\/blog\/index\.md/, "llms.txt should link the markdown blog index")
  assert.doesNotMatch(llms, /```/, "no code fences from the demos")
  assert.doesNotMatch(llms, /registerWorker|registerFunction/, "no SDK code from the demos")
  assert.doesNotMatch(llms, /\(opens in a new tab\)/, "no screen-reader-only labels")

  const agents = await read("AGENTS.md")
  assert.ok(agents.startsWith("# iii for AI Agents"), "AGENTS.md heading drifted")
  assert.match(agents, /## Homepage copy \(extracted from iii\.dev HTML\)/)
  assert.match(agents, /## Primitives \(wire-level\)/)
  assert.match(agents, /Last updated: \d{4}-\d{2}-\d{2}\n$/)
})

// --- roadmap -----------------------------------------------------------------

test("roadmap gallery and spec pages emit under /roadmap/ from the index.json feed", async () => {
  const feed = JSON.parse(await read("roadmap/index.json")) as { specs: { slug: string; url: string }[] }
  assert.ok(feed.specs.length > 0, "roadmap feed should list published specs")
  for (const spec of feed.specs) {
    assert.equal(spec.url, `/roadmap/${spec.slug}/`, "feed URLs must stay /roadmap/<slug>/")
    const page = path.join("roadmap", spec.slug, "index.html")
    assert.ok(has(page), `missing roadmap page for ${spec.slug}`)
    const html = await read(page)
    assert.match(
      html,
      new RegExp(`<link rel="canonical" href="https://iii\\.dev/roadmap/${spec.slug}/"/>`),
      `${page}: canonical should be the trailing-slash directory URL`,
    )
    assert.match(html, /<header class="fixed/, `${page}: shared site header missing`)
    assert.match(html, /<footer/, `${page}: shared site footer missing`)
  }
})
