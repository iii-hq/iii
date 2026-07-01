import fs from 'node:fs/promises'
import path from 'node:path'
import { readBlogPosts } from './blog-posts'
import { INDEXABLE_ROUTES, SITE_ORIGIN } from './routes'
import { readTechSpecs } from './tech-specs'

const OUT_PATH = path.resolve(process.cwd(), 'sitemap.xml')

const EXTRA_SITEMAP_PATHS: { path: string; priority: string }[] = [
  { path: '/llms.txt', priority: '0.6' },
  { path: '/AGENTS.md', priority: '0.6' },
]

function isoDate(d: Date = new Date()): string {
  return d.toISOString().slice(0, 10)
}

async function buildSitemap(): Promise<string> {
  const lastmod = isoDate()
  const routeUrls = INDEXABLE_ROUTES.map((route) => {
    const loc = `${SITE_ORIGIN}${route.path === '/' ? '/' : route.path}`
    const priority = route.path === '/' ? '1.0' : '0.7'
    return `  <url>
    <loc>${loc}</loc>
    <lastmod>${lastmod}</lastmod>
    <changefreq>weekly</changefreq>
    <priority>${priority}</priority>
  </url>`
  })

  const extraUrls = EXTRA_SITEMAP_PATHS.map(
    ({ path: p, priority }) => `  <url>
    <loc>${SITE_ORIGIN}${p}</loc>
    <lastmod>${lastmod}</lastmod>
    <changefreq>weekly</changefreq>
    <priority>${priority}</priority>
  </url>`,
  )

  const blogPosts = await readBlogPosts()
  const visiblePosts = blogPosts.filter((p) => !p.draft)
  const blogUrls: string[] = []
  if (visiblePosts.length > 0) {
    const newest = visiblePosts[0]
    const indexLastmod = isoDate(newest.updatedDate ?? newest.pubDate)
    blogUrls.push(`  <url>
    <loc>${SITE_ORIGIN}/blog/</loc>
    <lastmod>${indexLastmod}</lastmod>
    <changefreq>weekly</changefreq>
    <priority>0.7</priority>
  </url>`)
    for (const post of visiblePosts) {
      blogUrls.push(`  <url>
    <loc>${SITE_ORIGIN}/blog/${post.slug}/</loc>
    <lastmod>${isoDate(post.updatedDate ?? post.pubDate)}</lastmod>
    <changefreq>monthly</changefreq>
    <priority>0.6</priority>
  </url>`)
    }
  }

  // tech specs: the gallery at /tech-specs/ plus one URL per published spec
  // (drafts stay out of the sitemap, matching dist/index.json). lastmod uses
  // the spec's YYYY-MM date, pinned to the 1st.
  const techSpecs = (await readTechSpecs()).filter((s) => s.status !== 'draft')
  const techSpecUrls: string[] = []
  if (techSpecs.length > 0) {
    techSpecUrls.push(`  <url>
    <loc>${SITE_ORIGIN}/tech-specs/</loc>
    <lastmod>${techSpecs[0].date}-01</lastmod>
    <changefreq>weekly</changefreq>
    <priority>0.7</priority>
  </url>`)
    for (const spec of techSpecs) {
      techSpecUrls.push(`  <url>
    <loc>${SITE_ORIGIN}/tech-specs/${spec.slug}/</loc>
    <lastmod>${spec.date}-01</lastmod>
    <changefreq>monthly</changefreq>
    <priority>0.6</priority>
  </url>`)
    }
  }

  const urls = [...routeUrls, ...extraUrls, ...blogUrls, ...techSpecUrls].join('\n')

  return `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${urls}
</urlset>
`
}

async function generate() {
  await fs.writeFile(OUT_PATH, await buildSitemap(), 'utf8')
  console.log(`generated ${path.relative(process.cwd(), OUT_PATH)}`)
}

generate().catch((error) => {
  console.error('sitemap generation failed:', error)
  process.exitCode = 1
})
