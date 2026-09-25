import { dayLabel, monthLabel } from "@/lib/spec-dates"
import { getSpecs } from "@/lib/specs"

// /roadmap/index.json — the machine-readable spec list, same shape the Astro
// site emits (website/src/pages/roadmap/index.json.ts). Drafts are excluded
// by getSpecs. Prerendered, so `generatedAt` is the build time.
export const dynamic = "force-static"

export function GET(): Response {
  const feed = {
    generatedAt: new Date().toISOString(),
    specs: getSpecs().map((s) => ({
      slug: s.slug,
      title: s.title,
      tagline: s.tagline,
      date: s.date,
      month: monthLabel(s.date),
      dayLabel: dayLabel(s.date),
      tags: s.tags,
      status: s.status,
      hasDeck: !!s.deckUrl,
      url: `/roadmap/${s.slug}`,
    })),
  }
  return new Response(`${JSON.stringify(feed, null, 2)}\n`, {
    headers: { "Content-Type": "application/json" },
  })
}
