import 'server-only'

export type CommunityStats = {
  /** Formatted GitHub star count for iii-hq/iii, e.g. "18,818"; null when the API is unreachable. */
  stars: string | null
  /** Formatted approximate Discord member count, e.g. "2.4k"; null when the API is unreachable. */
  members: string | null
  /** Formatted Discord members online right now, e.g. "122"; null when the API is unreachable. */
  online: string | null
  /** Avatar URLs of a few members online now, from the public guild widget (same source as iii.dev's footer). */
  avatars: string[]
  /** Raw counts for the header's tick-up animation; null when unreachable. */
  starsCount: number | null
  membersCount: number | null
}

const REVALIDATE_SECONDS = 3600
const DISCORD_GUILD_ID = '1322278831184281721'

const compact = (n: number) =>
  new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 }).format(n).toLowerCase()

async function getJson<T>(url: string): Promise<T | null> {
  try {
    const res = await fetch(url, {
      headers: { Accept: 'application/json' },
      next: { revalidate: REVALIDATE_SECONDS },
      signal: AbortSignal.timeout(4000),
    })
    return res.ok ? ((await res.json()) as T) : null
  } catch {
    return null
  }
}

export async function getCommunityStats(): Promise<CommunityStats> {
  const [repo, invite, widget] = await Promise.all([
    getJson<{ stargazers_count?: number }>('https://api.github.com/repos/iii-hq/iii'),
    getJson<{ approximate_member_count?: number; approximate_presence_count?: number }>(
      'https://discord.com/api/v10/invites/iiidev?with_counts=true',
    ),
    getJson<{ presence_count?: number; members?: { avatar_url?: string }[] }>(
      `https://discord.com/api/guilds/${DISCORD_GUILD_ID}/widget.json`,
    ),
  ])
  const stars = repo?.stargazers_count
  const members = invite?.approximate_member_count
  const online = invite?.approximate_presence_count ?? widget?.presence_count
  const avatars = (widget?.members ?? [])
    .map((m) => m.avatar_url)
    .filter((url): url is string => typeof url === 'string' && url.length > 0)
    .slice(0, 5)
  return {
    stars: typeof stars === 'number' ? stars.toLocaleString('en-US') : null,
    members: typeof members === 'number' ? compact(members) : null,
    online: typeof online === 'number' ? online.toLocaleString('en-US') : null,
    avatars,
    starsCount: typeof stars === 'number' ? stars : null,
    membersCount: typeof members === 'number' ? members : null,
  }
}
