/**
 * The people behind the repositories the site showcases, read from GitHub's contributors API. Pure: the fetching
 * lives in scripts/generate-contributors.ts, which writes content/contributors/data.json for the landing page.
 */

import { links } from './site'

export type Contributor = {
  /** GitHub user id: stable across renames, so it is the key the merge works on. */
  id: number
  login: string
  /** Avatar without a size: the component appends `s=` for the pixel size it draws. */
  avatarUrl: string
  url: string
  /** Commits across every repository in `repos`. */
  contributions: number
  /** The showcased repositories this person has landed a commit in, e.g. "iii-hq/iii". */
  repos: string[]
}

export type ContributorsData = {
  /** The repositories read, in the order they were asked for. */
  repos: string[]
  /** When the API was read (ISO 8601). */
  fetchedAt: string
  /** Deduplicated people, bots removed, most commits first. */
  contributors: Contributor[]
}

/** One entry of GET /repos/{owner}/{repo}/contributors, the fields this site uses. */
export type GitHubContributor = {
  id: number
  login: string
  avatar_url: string
  html_url: string
  type: 'User' | 'Bot' | string
  contributions: number
}

/** "owner/name" from a GitHub repository URL, e.g. https://github.com/iii-hq/iii → iii-hq/iii. */
export function repoSlug(url: string): string {
  const match = /github\.com\/([^/]+\/[^/#?]+)/.exec(url)
  if (!match) throw new Error(`Not a GitHub repository URL: ${url}`)
  return match[1].replace(/\.git$/, '')
}

/** The repositories the site links to (lib/site.ts): the engine and the worker registry. */
export const CONTRIBUTOR_REPOS: readonly string[] = [links.github, links.workers].map(repoSlug)

/**
 * Automation accounts GitHub reports as type "User". `actions-user` is the account workflows commit as when
 * they are not given a token of their own; `web-flow` signs GitHub's own merge commits.
 */
const AUTOMATION_LOGINS = new Set(['actions-user', 'web-flow'])

/** Accounts that are not a person: the organisation's own account, which commits on its behalf. */
const ORG_LOGINS = new Set(['iii-headquarters'])

/** True for GitHub Apps (type "Bot" or a `[bot]` suffix) and the automation accounts above. */
export function isBot(entry: Pick<GitHubContributor, 'login' | 'type'>): boolean {
  const login = entry.login.toLowerCase()
  return entry.type === 'Bot' || login.endsWith('[bot]') || AUTOMATION_LOGINS.has(login)
}

/** True for anything the list leaves out: bots, and accounts that are not a person. */
export function isExcluded(entry: Pick<GitHubContributor, 'login' | 'type'>): boolean {
  return isBot(entry) || ORG_LOGINS.has(entry.login.toLowerCase())
}

/**
 * Merges the contributor lists of several repositories into one: one row per person (by GitHub user id),
 * commits summed, bots and the organisation's account dropped, most commits first with the login as the tiebreak so the order is stable.
 */
export function mergeContributors(lists: { repo: string; entries: GitHubContributor[] }[]): Contributor[] {
  const byId = new Map<number, Contributor>()
  for (const { repo, entries } of lists) {
    for (const entry of entries) {
      if (isExcluded(entry)) continue
      const seen = byId.get(entry.id)
      if (seen) {
        seen.contributions += entry.contributions
        if (!seen.repos.includes(repo)) seen.repos.push(repo)
      } else {
        byId.set(entry.id, {
          id: entry.id,
          login: entry.login,
          avatarUrl: entry.avatar_url,
          url: entry.html_url,
          contributions: entry.contributions,
          repos: [repo],
        })
      }
    }
  }
  return [...byId.values()].sort(
    (a, b) => b.contributions - a.contributions || a.login.localeCompare(b.login, 'en', { sensitivity: 'base' }),
  )
}

/** URL the site serves the list at (public/data/contributors.json), refreshed daily without rebuilding the site. */
export const CONTRIBUTORS_URL = '/data/contributors.json'

/** True for a list worth drawing: the shape generate-contributors writes, with at least one person in it. */
export function isContributorsData(value: unknown): value is ContributorsData {
  if (typeof value !== 'object' || value === null) return false
  const { repos, contributors } = value as Partial<ContributorsData>
  return (
    Array.isArray(repos) &&
    Array.isArray(contributors) &&
    contributors.length > 0 &&
    contributors.every(
      (c) =>
        typeof c?.id === 'number' &&
        typeof c.login === 'string' &&
        typeof c.avatarUrl === 'string' &&
        typeof c.url === 'string' &&
        typeof c.contributions === 'number',
    )
  )
}
