// Date labels for the roadmap. The feed keeps the Astro contract (lowercase
// "2026 · june" / "jun 29"); the pages use the site's sentence-case voice.

const MONTHS = [
  "january",
  "february",
  "march",
  "april",
  "may",
  "june",
  "july",
  "august",
  "september",
  "october",
  "november",
  "december",
]

const parts = (iso: string) => iso.match(/^(\d{4})-(\d{2})(?:-(\d{2}))?$/)

/** "2026-06-29" → "2026 · june" (the feed's month group; the raw string when unparseable) */
export function monthLabel(iso: string) {
  const m = parts(iso)
  const name = m && MONTHS[Number(m[2]) - 1]
  return m && name ? `${m[1]} · ${name}` : iso
}

/** "2026-06-29" → "jun 29"; null when the date has no day */
export function dayLabel(iso: string) {
  const m = parts(iso)
  const name = m?.[3] && MONTHS[Number(m[2]) - 1]
  return m && name ? `${name.slice(0, 3)} ${Number(m[3])}` : null
}

/** "2026-06-29" → "2026-06", the key the timeline groups by */
export function monthKey(iso: string) {
  return iso.slice(0, 7)
}

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1)

/** "2026-06" or "2026-06-29" → "June 2026" */
export function monthHeading(iso: string) {
  const m = parts(iso)
  const name = m && MONTHS[Number(m[2]) - 1]
  return m && name ? `${cap(name)} ${m[1]}` : iso
}

/** "2026-06-29" → "Jun 29" (the day within a month group) */
export function dayHeading(iso: string) {
  const d = dayLabel(iso)
  return d ? cap(d) : iso
}

const LONG = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" })

/** "2026-06-29" → "Jun 29, 2026" */
export function formatSpecDate(iso: string) {
  const date = new Date(`${iso}T00:00:00Z`)
  return Number.isNaN(date.getTime()) ? iso : LONG.format(date)
}
