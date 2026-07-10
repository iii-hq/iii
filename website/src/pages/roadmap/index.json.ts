// /roadmap/index.json — the machine-readable spec list feeding the landing
// page's #tech-specs timeline. Drafts are excluded (they still build and show
// muted in the gallery), matching the sitemap.
import { readSpecs } from '../../../roadmap/scripts/manifest.mjs'

export function GET(): Response {
  const { specs } = readSpecs()
  const feed = {
    generatedAt: new Date().toISOString(),
    specs: specs
      .filter((s) => s.status !== 'draft')
      .map(({ slug, title, tagline, date, month, dayLabel, tags, status, hasDeck }) => ({
        slug,
        title,
        tagline,
        date,
        month,
        dayLabel,
        tags,
        status,
        hasDeck,
        url: `/roadmap/${slug}/`,
      })),
  }
  return new Response(`${JSON.stringify(feed, null, 2)}\n`, {
    headers: { 'Content-Type': 'application/json' },
  })
}
