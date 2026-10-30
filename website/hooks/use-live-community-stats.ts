'use client'

import { useEffect, useState } from 'react'

import { formatters } from '@/components/site/count-up'
import type { CommunityStats } from '@/lib/community'

export type LiveCounts = { stars: number | null; members: number | null }

const CACHE_KEY = 'iii:community-counts'
/** GitHub serves the repository with `max-age=60`, so a fresher reading would be the same number. */
const CACHE_TTL_MS = 60 * 1000

function readCache(): LiveCounts | null {
  try {
    const cached = JSON.parse(localStorage.getItem(CACHE_KEY) ?? 'null') as (LiveCounts & { at: number }) | null
    return cached && Date.now() - cached.at < CACHE_TTL_MS ? cached : null
  } catch {
    return null
  }
}

function writeCache(counts: LiveCounts) {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify({ ...counts, at: Date.now() }))
  } catch {
    return
  }
}

async function getCount<T>(url: string, pick: (body: T) => unknown): Promise<number | null> {
  try {
    const res = await fetch(url, { headers: { Accept: 'application/json' }, signal: AbortSignal.timeout(5000) })
    if (!res.ok) return null
    const count = pick((await res.json()) as T)
    return typeof count === 'number' ? count : null
  } catch {
    return null
  }
}

async function loadCounts(): Promise<LiveCounts | null> {
  const [stars, members] = await Promise.all([
    getCount<{ stargazers_count?: number }>('https://api.github.com/repos/iii-hq/iii', (repo) => repo.stargazers_count),
    getCount<{ approximate_member_count?: number }>(
      'https://discord.com/api/v10/invites/iiidev?with_counts=true',
      (invite) => invite.approximate_member_count,
    ),
  ])
  if (stars === null && members === null) return null
  const counts = { stars, members }
  writeCache(counts)
  return counts
}

/** The request in flight, shared so the header and the landing page's star button read GitHub and Discord once. */
let pending: Promise<LiveCounts | null> | null = null

function readLiveCounts(): Promise<LiveCounts | null> {
  const cached = readCache()
  if (cached) return Promise.resolve(cached)
  pending ??= loadCounts().finally(() => {
    pending = null
  })
  return pending
}

/**
 * The GitHub star and Discord member counts, read in the browser on load and again whenever the tab comes back into
 * view, so a star shows up without a rebuild. `initial` is the build-time reading: it is what renders first, and what
 * stays when an API is unreachable or rate limited.
 */
export function useLiveCounts(initial: LiveCounts): LiveCounts {
  const [live, setLive] = useState<LiveCounts | null>(null)

  useEffect(() => {
    let active = true
    const refresh = () =>
      readLiveCounts().then((counts) => {
        if (active && counts) setLive(counts)
      })
    const onVisible = () => document.visibilityState === 'visible' && refresh()
    refresh()
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      active = false
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [])

  return { stars: live?.stars ?? initial.stars, members: live?.members ?? initial.members }
}

export function useLiveCommunityStats(initial: CommunityStats): CommunityStats {
  const { stars, members } = useLiveCounts({ stars: initial.starsCount, members: initial.membersCount })
  return {
    ...initial,
    stars: stars === null ? null : formatters.plain(stars),
    starsCount: stars,
    members: members === null ? null : formatters.compact(members),
    membersCount: members,
  }
}
