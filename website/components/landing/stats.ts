import 'server-only'

import { getCommunityStats } from '@/lib/community'

export type PageStats = {
  stars: number | null
  contributors: number | null
  /** Workers listed at workers.iii.dev, read from the registry's own badge. */
  workers: number
  /** Adoption, as the 2026-10-05 sync asked for: the SDK on each package registry, plus the engine image. */
  downloads: {
    npmWeek: number
    pypiWeek: number
    crates90d: number
    dockerPulls: number
  }
}

/**
 * Last good reading of each source, checked 2026-10-08. Used only when an API is unreachable, so the
 * section never shows a dash where a real figure belongs.
 */
const FALLBACK = {
  workers: 98,
  contributors: 54,
  npmWeek: 31_901,
  pypiWeek: 2_857,
  crates90d: 45_742,
  dockerPulls: 118_713,
}

async function getJson<T>(url: string): Promise<T | null> {
  try {
    const res = await fetch(url, {
      headers: { Accept: 'application/json', 'User-Agent': 'iii.dev (https://iii.dev)' },
      next: { revalidate: 3600 },
      signal: AbortSignal.timeout(4000),
    })
    return res.ok ? ((await res.json()) as T) : null
  } catch {
    return null
  }
}

/** Contributor count from the GitHub API pagination header; null when unreachable. */
async function getContributors(): Promise<number | null> {
  try {
    const res = await fetch('https://api.github.com/repos/iii-hq/iii/contributors?per_page=1&anon=true', {
      headers: { Accept: 'application/json' },
      next: { revalidate: 3600 },
      signal: AbortSignal.timeout(4000),
    })
    if (!res.ok) return null
    const last = res.headers.get('link')?.match(/[?&]page=(\d+)>; rel="last"/)
    return last ? Number(last[1]) : 1
  } catch {
    return null
  }
}

/**
 * Worker count from the registry's badge (the same one the README embeds). The registry has no JSON
 * endpoint for this yet, so the figure comes out of the SVG's accessible label: `iii workers 98`.
 */
async function getWorkers(): Promise<number | null> {
  try {
    const res = await fetch('https://workers.iii.dev/badge/workers.svg', {
      headers: { Accept: 'image/svg+xml' },
      next: { revalidate: 3600 },
      signal: AbortSignal.timeout(4000),
    })
    if (!res.ok) return null
    const count = (await res.text()).match(/aria-label="iii workers (\d+)"/)
    return count ? Number(count[1]) : null
  } catch {
    return null
  }
}

/** `iii-sdk` on npm, PyPI and crates.io, and the `iiidev/iii` engine image on Docker Hub. */
async function getDownloads(): Promise<PageStats['downloads']> {
  const [npm, pypi, crates, docker] = await Promise.all([
    getJson<{ downloads: number }>('https://api.npmjs.org/downloads/point/last-week/iii-sdk'),
    getJson<{ data: { last_week: number } }>('https://pypistats.org/api/packages/iii-sdk/recent'),
    getJson<{ crate: { recent_downloads: number } }>('https://crates.io/api/v1/crates/iii-sdk'),
    getJson<{ pull_count: number }>('https://hub.docker.com/v2/repositories/iiidev/iii/'),
  ])
  return {
    npmWeek: npm?.downloads ?? FALLBACK.npmWeek,
    pypiWeek: pypi?.data.last_week ?? FALLBACK.pypiWeek,
    crates90d: crates?.crate.recent_downloads ?? FALLBACK.crates90d,
    dockerPulls: docker?.pull_count ?? FALLBACK.dockerPulls,
  }
}

export async function getPageStats(): Promise<PageStats> {
  const [community, workers, contributors, downloads] = await Promise.all([
    getCommunityStats(),
    getWorkers(),
    getContributors(),
    getDownloads(),
  ])
  return {
    stars: community.starsCount,
    workers: workers ?? FALLBACK.workers,
    contributors: contributors ?? FALLBACK.contributors,
    downloads,
  }
}
