import { getSpecs } from "@/lib/specs"

// The landing preview reads the tech specs straight from the repo (see
// src/lib/specs.ts), the same source the /roadmap pages and the
// /roadmap/index.json feed use. No network fetch at build time.
export type RoadmapSpec = {
  slug: string
  title: string
  tagline?: string
  /** ISO date, e.g. "2026-06-29" */
  date: string
  tags?: string[]
  /** the spec ships an interactive deck */
  hasDeck?: boolean
}

const LIMIT = 4

/** Latest published specs for the landing preview; `[]` when none can be read. */
export async function getLatestSpecs(): Promise<RoadmapSpec[]> {
  try {
    return getSpecs()
      .slice(0, LIMIT)
      .map((s) => ({
        slug: s.slug,
        title: s.title,
        tagline: s.tagline,
        date: s.date,
        tags: s.tags,
        hasDeck: !!s.deckUrl,
      }))
  } catch {
    return []
  }
}

export function specHref(spec: RoadmapSpec) {
  return `/roadmap/${spec.slug}`
}

const DATE_FORMAT = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  year: "numeric",
  timeZone: "UTC",
})

/** "2026-06-29" → "Jun 29, 2026". Falls back to the raw string for anything else. */
export function formatSpecDate(iso: string) {
  const date = new Date(`${iso}T00:00:00Z`)
  return Number.isNaN(date.getTime()) ? iso : DATE_FORMAT.format(date)
}
