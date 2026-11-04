/**
 * Seasonal dressings for the iii mark in the header and footer. Each season has a date window in the reader's local
 * time, so a mark turns itself on and off without a deploy. `?season=<name>` previews one on any date, and
 * `?season=off` turns them off; the choice sticks for the rest of the browsing session.
 */
export const SEASONS = ['thanksgiving'] as const
export type Season = (typeof SEASONS)[number]

/** US Thanksgiving: the fourth Thursday of November. */
export function thanksgivingDay(year: number): Date {
  const firstOfNovember = new Date(year, 10, 1)
  const firstThursday = 1 + ((4 - firstOfNovember.getDay() + 7) % 7)
  return new Date(year, 10, firstThursday + 21)
}

/** Thanksgiving week: from the Monday before Thanksgiving to the end of the Sunday after it. End is exclusive. */
export function thanksgivingWindow(year: number): { start: Date; end: Date } {
  const day = thanksgivingDay(year)
  return {
    start: new Date(year, 10, day.getDate() - 3),
    end: new Date(year, 10, day.getDate() + 4),
  }
}

/** The season showing on `date`, if any. */
export function seasonOn(date: Date): Season | null {
  const { start, end } = thanksgivingWindow(date.getFullYear())
  if (date >= start && date < end) return 'thanksgiving'
  return null
}

/**
 * The preview override in a query string: a season name, `null` for `off`, or `undefined` when the query says
 * nothing (or names a season that does not exist).
 */
export function seasonFromQuery(search: string): Season | null | undefined {
  const value = new URLSearchParams(search).get('season')
  if (value === 'off') return null
  return SEASONS.find((season) => season === value)
}

const OVERRIDE_KEY = 'iii-season'

/**
 * TEMPORARY, for the team preview: shows this season every day, ignoring the date window. Set back to `null` before
 * merging so the mark only appears during Thanksgiving week. `?season=off` still turns it off.
 */
const ALWAYS_ON: Season | null = 'thanksgiving'

/** The season to show in this browser right now: a preview override from the URL or session, else the date. */
export function currentSeason(): Season | null {
  try {
    const fromQuery = seasonFromQuery(window.location.search)
    if (fromQuery !== undefined) {
      window.sessionStorage.setItem(OVERRIDE_KEY, fromQuery ?? 'off')
      return fromQuery
    }
    const stored = window.sessionStorage.getItem(OVERRIDE_KEY)
    if (stored === 'off') return null
    const fromSession = SEASONS.find((season) => season === stored)
    if (fromSession) return fromSession
  } catch {
    // Storage blocked: fall back to the date.
  }
  return ALWAYS_ON ?? seasonOn(new Date())
}
