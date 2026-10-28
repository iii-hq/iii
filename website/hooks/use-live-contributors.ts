'use client'

import { useEffect, useState } from 'react'

import { CONTRIBUTORS_URL, type ContributorsData, isContributorsData } from '@/lib/contributors'

/** The request in flight, shared so the section's count and its row read the file once. */
let pending: Promise<ContributorsData | null> | null = null

async function loadContributors(): Promise<ContributorsData | null> {
  try {
    const res = await fetch(CONTRIBUTORS_URL, { cache: 'no-cache', signal: AbortSignal.timeout(5000) })
    if (!res.ok) return null
    const data: unknown = await res.json()
    return isContributorsData(data) ? data : null
  } catch {
    return null
  }
}

/**
 * The contributors list from /data/contributors.json, which a daily workflow refreshes on its own without
 * rebuilding the site. `initial` is the list the page was built with: it renders first, and it stays when the file
 * cannot be read or is not a list worth drawing.
 */
export function useLiveContributors(initial: ContributorsData): ContributorsData {
  const [live, setLive] = useState<ContributorsData | null>(null)

  useEffect(() => {
    let active = true
    pending ??= loadContributors()
    pending.then((data) => {
      if (active && data) setLive(data)
    })
    return () => {
      active = false
    }
  }, [])

  return live ?? initial
}
