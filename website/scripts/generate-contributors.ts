// Writes content/contributors/data.json, the people the landing page's Contributors section draws, before every
// build and dev server (see package.json). The list is read from GitHub's contributors API for every repository
// the site showcases (lib/contributors.ts CONTRIBUTOR_REPOS), merged into one list with bots removed.
//
// When any repository cannot be read, the whole list comes from the snapshot committed next to this data
// (content/contributors/snapshot.json), so a build never fails or ships an empty section because GitHub was
// slow or rate limited. CONTRIBUTORS_OFFLINE=1 skips the network; GITHUB_TOKEN, when set, lifts the
// unauthenticated rate limit of 60 requests an hour.
//
// Refreshing the snapshot: tsx scripts/generate-contributors.ts --snapshot
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"

import {
  CONTRIBUTOR_REPOS,
  type ContributorsData,
  type GitHubContributor,
  mergeContributors,
} from "../lib/contributors"

const here = dirname(fileURLToPath(import.meta.url))
export const DATA = resolve(here, "../content/contributors/data.json")
export const SNAPSHOT = resolve(here, "../content/contributors/snapshot.json")
const PER_PAGE = 100
/** A repository with more pages than this is not one of ours; it also bounds the requests a build makes. */
const MAX_PAGES = 10

function headers(): Record<string, string> {
  const h: Record<string, string> = {
    Accept: "application/vnd.github+json",
    "User-Agent": "iii.dev (https://iii.dev)",
  }
  if (process.env.GITHUB_TOKEN) h.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`
  return h
}

/** Every page of a repository's contributors, or null when any page cannot be read. */
export async function fetchRepoContributors(repo: string): Promise<GitHubContributor[] | null> {
  const entries: GitHubContributor[] = []
  for (let page = 1; page <= MAX_PAGES; page++) {
    const url = `https://api.github.com/repos/${repo}/contributors?per_page=${PER_PAGE}&page=${page}`
    let res: Response
    try {
      res = await fetch(url, { headers: headers(), signal: AbortSignal.timeout(8000) })
    } catch (error) {
      console.warn(`contributors: ${repo} page ${page} unreachable (${(error as Error).message})`)
      return null
    }
    if (!res.ok) {
      console.warn(`contributors: ${repo} page ${page} returned ${res.status}`)
      return null
    }
    // 204 No Content: GitHub's answer for a repository with no commits yet.
    if (res.status === 204) break
    let batch: unknown
    try {
      batch = await res.json()
    } catch (error) {
      console.warn(`contributors: ${repo} page ${page} is not JSON (${(error as Error).message})`)
      return null
    }
    if (!Array.isArray(batch)) {
      console.warn(`contributors: ${repo} page ${page} is not a list`)
      return null
    }
    entries.push(...(batch as GitHubContributor[]))
    if (batch.length < PER_PAGE) break
  }
  return entries
}

async function fetchAll(): Promise<ContributorsData | null> {
  const lists: { repo: string; entries: GitHubContributor[] }[] = []
  for (const repo of CONTRIBUTOR_REPOS) {
    const entries = await fetchRepoContributors(repo)
    if (entries === null) return null
    lists.push({ repo, entries })
  }
  return {
    repos: [...CONTRIBUTOR_REPOS],
    fetchedAt: new Date().toISOString(),
    contributors: mergeContributors(lists),
  }
}

function readSnapshot(): ContributorsData {
  if (!existsSync(SNAPSHOT)) throw new Error(`contributors: no snapshot at ${SNAPSHOT}`)
  return JSON.parse(readFileSync(SNAPSHOT, "utf8")) as ContributorsData
}

async function main() {
  const snapshot = process.argv.includes("--snapshot")
  const offline = process.env.CONTRIBUTORS_OFFLINE === "1"
  const live = offline ? null : await fetchAll()
  const data = live ?? readSnapshot()
  const source = live ? "github" : offline ? "snapshot (offline)" : "snapshot (GitHub unreachable)"

  mkdirSync(dirname(DATA), { recursive: true })
  const json = `${JSON.stringify(data, null, 2)}\n`
  writeFileSync(DATA, json)
  if (snapshot && live) writeFileSync(SNAPSHOT, json)
  console.log(`contributors: ${data.contributors.length} people from ${data.repos.join(", ")} (${source})`)
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error)
    process.exit(1)
  })
}
