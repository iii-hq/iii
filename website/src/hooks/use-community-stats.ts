"use client"

import { useSyncExternalStore } from "react"

// GitHub stars + Discord counts, fetched once per page and shared by every
// consumer (nav, mobile menu, footer Discord widget). Ported from SiteNav.astro
// and the "Footer Discord widget" in 02-page-behaviors.html: same endpoints,
// same formatting, same 5-minute Discord refresh. Every request fails silently.

const GITHUB_URL = "https://api.github.com/repos/iii-hq/iii"
const DISCORD_GUILD_ID = "1322278831184281721"
const DISCORD_WIDGET_URL = `https://discord.com/api/guilds/${DISCORD_GUILD_ID}/widget.json`
// The invite endpoint exposes approximate_member_count (total members), which
// the widget endpoint does not. Used for the nav / mobile menu total.
const DISCORD_INVITE_URL = "https://discord.com/api/v10/invites/iiidev?with_counts=true"
const DISCORD_REFRESH_MS = 5 * 60 * 1000

export type DiscordMember = { id: string; username: string; avatarUrl?: string }

/** `undefined` = still loading, `null` = the request failed. */
export type CommunityStats = {
  stars: number | null | undefined
  /** total members (invite endpoint), falling back to the online count */
  members: number | null | undefined
  /** members online right now (widget endpoint) */
  online: number | null | undefined
  avatars: DiscordMember[]
}

let state: CommunityStats = { stars: undefined, members: undefined, online: undefined, avatars: [] }
const listeners = new Set<() => void>()
let started = false

// The last counts seen, so a hard reload shows real numbers at once instead
// of a placeholder that then jumps. Refreshed by every successful fetch.
const CACHE_KEY = "iii_community_stats"
type Cached = { stars?: number; members?: number }

function readCache(): Cached {
  try {
    return JSON.parse(localStorage.getItem(CACHE_KEY) ?? "{}") as Cached
  } catch {
    return {}
  }
}

function writeCache() {
  try {
    const cached: Cached = {}
    if (typeof state.stars === "number") cached.stars = state.stars
    if (typeof state.members === "number") cached.members = state.members
    localStorage.setItem(CACHE_KEY, JSON.stringify(cached))
  } catch {
    // storage unavailable (private mode, quota): the live fetch still fills in
  }
}

function set(patch: Partial<CommunityStats>) {
  state = { ...state, ...patch }
  for (const l of listeners) l()
}

async function fetchGitHub() {
  try {
    const res = await fetch(GITHUB_URL)
    if (!res.ok) throw new Error(`GitHub ${res.status}`)
    const data = await res.json()
    set({ stars: typeof data?.stargazers_count === "number" ? data.stargazers_count : null })
    writeCache()
  } catch {
    set({ stars: state.stars ?? null })
  }
}

type WidgetMember = { id?: string; username?: string; avatar_url?: string }

async function fetchDiscord() {
  const [widget, invite] = await Promise.all([
    fetch(DISCORD_WIDGET_URL, { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : null))
      .catch(() => null),
    fetch(DISCORD_INVITE_URL, { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : null))
      .catch(() => null),
  ])
  const online = widget ? Number(widget.presence_count || 0) : null
  const total = invite ? Number(invite.approximate_member_count || 0) : 0
  const avatars: DiscordMember[] = widget
    ? ((widget.members ?? []) as WidgetMember[]).filter(Boolean).map((m, i) => ({
        id: m.id ?? String(i),
        username: m.username ?? "",
        avatarUrl: m.avatar_url || undefined,
      }))
    : []
  set({ online, members: total || online || state.members || null, avatars })
  writeCache()
}

function start() {
  if (started) return
  started = true
  const cached = readCache()
  if (cached.stars != null || cached.members != null) set(cached)
  void fetchGitHub()
  void fetchDiscord()
  setInterval(fetchDiscord, DISCORD_REFRESH_MS)
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  start()
  return () => {
    listeners.delete(listener)
  }
}

const getSnapshot = () => state
const serverSnapshot: CommunityStats = { stars: undefined, members: undefined, online: undefined, avatars: [] }
const getServerSnapshot = () => serverSnapshot

export function useCommunityStats(): CommunityStats {
  return useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot)
}

/** 2400 → "2.4k", 18802 → "19k", 118 → "118" (Discord counts). */
export function formatCount(n: number) {
  if (n >= 1000) return `${(n / 1000).toFixed(n >= 10000 ? 0 : 1).replace(/\.0$/, "")}k`
  return String(n)
}

/** 18802 → "18,802" (GitHub stars). */
export function formatStars(n: number) {
  return n.toLocaleString("en-US")
}
