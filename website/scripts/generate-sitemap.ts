import fs from 'node:fs/promises'
import path from 'node:path'
import { INDEXABLE_ROUTES, SITE_ORIGIN } from './routes'

const OUT_PATH = path.resolve(process.cwd(), 'sitemap.xml')

const EXTRA_SITEMAP_PATHS: { path: string; priority: string }[] = [
  { path: '/llms.txt', priority: '0.6' },
  { path: '/AGENTS.md', priority: '0.6' },
]

function isoDate(): string {
  return new Date().toISOString().slice(0, 10)
}

function buildSitemap(): string {
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

  const urls = [...routeUrls, ...extraUrls].join('\n')

  return `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${urls}
</urlset>
`
}

async function generate() {
  await fs.writeFile(OUT_PATH, buildSitemap(), 'utf8')
  console.log(`generated ${path.relative(process.cwd(), OUT_PATH)}`)
}

generate().catch((error) => {
  console.error('sitemap generation failed:', error)
  process.exitCode = 1
})
