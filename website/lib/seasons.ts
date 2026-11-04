/**
 * Seasonal dressings for the iii mark in the header and footer. Each season has a date window in the reader's local
 * time, so a mark turns itself on and off without a deploy. `?season=<name>` previews one on any date, and
 * `?season=off` turns them off; the choice sticks for the rest of the browsing session.
 */
/** In priority order: when two seasons overlap (Diwali falls in early November), the first one shows. */
export const SEASONS = ['diwali', 'thanksgiving'] as const
export type Season = (typeof SEASONS)[number]

/** US Thanksgiving: the fourth Thursday of November. */
export function thanksgivingDay(year: number): Date {
  const firstOfNovember = new Date(year, 10, 1)
  const firstThursday = 1 + ((4 - firstOfNovember.getDay() + 7) % 7)
  return new Date(year, 10, firstThursday + 21)
}

/**
 * Years whose Thanksgiving mark starts before Thanksgiving week. 2026 starts on Nov 5 so the team can see it on the
 * live site; it still ends with the week, on its own.
 */
const EARLY_START: Record<number, Date> = { 2026: new Date(2026, 10, 5) }

/**
 * Thanksgiving week: from the Monday before Thanksgiving (or the year's early start) to the end of the Sunday after
 * it. End is exclusive.
 */
export function thanksgivingWindow(year: number): { start: Date; end: Date } {
  const day = thanksgivingDay(year)
  return {
    start: EARLY_START[year] ?? new Date(year, 10, day.getDate() - 3),
    end: new Date(year, 10, day.getDate() + 4),
  }
}

/**
 * Diwali follows the Hindu lunar calendar, so its dates come from a table: the five-day festival from Dhanteras to
 * Bhai Dooj, inclusive (Diwali itself, Lakshmi Puja, is the third day). Source: Drik Panchang's Diwali calendar,
 * drikpanchang.com/diwali/diwali-puja-calendar.html, New Delhi. Add a year here to keep it running.
 */
const DIWALI: Record<number, { from: [month: number, day: number]; to: [month: number, day: number] }> = {
  2026: { from: [11, 6], to: [11, 11] },
  2027: { from: [10, 27], to: [10, 31] },
  2028: { from: [10, 15], to: [10, 19] },
  2029: { from: [11, 4], to: [11, 7] },
  2030: { from: [10, 24], to: [10, 28] },
}

/** The Diwali festival in `year`, start inclusive and end exclusive, or null for a year not in the table. */
export function diwaliWindow(year: number): { start: Date; end: Date } | null {
  const days = DIWALI[year]
  if (!days) return null
  return {
    start: new Date(year, days.from[0] - 1, days.from[1]),
    end: new Date(year, days.to[0] - 1, days.to[1] + 1),
  }
}

/** The season showing on `date`, if any; Diwali wins where it overlaps Thanksgiving. */
export function seasonOn(date: Date): Season | null {
  const diwali = diwaliWindow(date.getFullYear())
  if (diwali && date >= diwali.start && date < diwali.end) return 'diwali'
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
  return seasonOn(new Date())
}
