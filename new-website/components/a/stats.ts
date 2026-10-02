import 'server-only'

import { getCommunityStats } from '@/lib/community'

export type PageStats = {
  stars: number | null
  contributors: number | null
  /** Workers listed at workers.iii.dev, counted by hand on 2026-10-03. */
  workers: number
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

export async function getPageStats(): Promise<PageStats> {
  const [community, contributors] = await Promise.all([getCommunityStats(), getContributors()])
  return { stars: community.starsCount, contributors, workers: 96 }
}
