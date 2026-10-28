import assert from "node:assert/strict"
import { afterEach, describe, it } from "node:test"

import {
  CONTRIBUTOR_REPOS,
  type GitHubContributor,
  isBot,
  isContributorsData,
  isExcluded,
  mergeContributors,
  repoSlug,
} from "../lib/contributors"
import { fetchRepoContributors } from "./generate-contributors"

const user = (id: number, login: string, contributions: number, type = "User"): GitHubContributor => ({
  id,
  login,
  avatar_url: `https://avatars.githubusercontent.com/u/${id}?v=4`,
  html_url: `https://github.com/${login}`,
  type,
  contributions,
})

describe("repositories", () => {
  it("reads owner/name from the site's GitHub links", () => {
    assert.equal(repoSlug("https://github.com/iii-hq/iii"), "iii-hq/iii")
    assert.equal(repoSlug("https://github.com/iii-hq/workers/"), "iii-hq/workers")
    assert.equal(repoSlug("https://github.com/iii-hq/iii.git"), "iii-hq/iii")
    assert.throws(() => repoSlug("https://iii.dev/docs"))
  })

  it("showcases the engine and the worker registry", () => {
    assert.deepEqual([...CONTRIBUTOR_REPOS], ["iii-hq/iii", "iii-hq/workers"])
  })
})

describe("bots", () => {
  it("drops GitHub Apps and automation accounts, keeps people", () => {
    assert.ok(isBot({ login: "github-actions[bot]", type: "Bot" }))
    assert.ok(isBot({ login: "dependabot[bot]", type: "Bot" }))
    assert.ok(isBot({ login: "some-app[bot]", type: "User" }))
    assert.ok(isBot({ login: "actions-user", type: "User" }))
    assert.ok(isBot({ login: "web-flow", type: "User" }))
    assert.ok(!isBot({ login: "kebot", type: "User" }))
    assert.ok(!isBot({ login: "ytallo", type: "User" }))
  })

  it("leaves out the organisation's own account, which is not a person", () => {
    assert.ok(isExcluded({ login: "iii-headquarters", type: "User" }))
    assert.ok(isExcluded({ login: "github-actions[bot]", type: "Bot" }))
    assert.ok(!isBot({ login: "iii-headquarters", type: "User" }))
    assert.ok(!isExcluded({ login: "ytallo", type: "User" }))
  })
})

describe("merge", () => {
  it("dedupes by user id across repositories, sums commits and lists the repositories", () => {
    const merged = mergeContributors([
      { repo: "iii-hq/iii", entries: [user(1, "ana", 10), user(2, "ben", 3)] },
      { repo: "iii-hq/workers", entries: [user(1, "ana", 5), user(3, "cy", 1)] },
    ])
    assert.deepEqual(
      merged.map((c) => [c.login, c.contributions, c.repos]),
      [
        ["ana", 15, ["iii-hq/iii", "iii-hq/workers"]],
        ["ben", 3, ["iii-hq/iii"]],
        ["cy", 1, ["iii-hq/workers"]],
      ],
    )
  })

  it("keeps the fields the page draws", () => {
    const [ana] = mergeContributors([{ repo: "iii-hq/iii", entries: [user(1, "ana", 10)] }])
    assert.deepEqual(ana, {
      id: 1,
      login: "ana",
      avatarUrl: "https://avatars.githubusercontent.com/u/1?v=4",
      url: "https://github.com/ana",
      contributions: 10,
      repos: ["iii-hq/iii"],
    })
  })

  it("removes bots and orders by commits, then login", () => {
    const merged = mergeContributors([
      {
        repo: "iii-hq/iii",
        entries: [
          user(9, "github-actions[bot]", 500, "Bot"),
          user(8, "actions-user", 400),
          user(7, "iii-headquarters", 300),
          user(1, "zed", 2),
          user(2, "Amy", 2),
          user(3, "max", 7),
        ],
      },
    ])
    assert.deepEqual(
      merged.map((c) => c.login),
      ["max", "Amy", "zed"],
    )
  })

  it("is empty when nothing was read", () => {
    assert.deepEqual(mergeContributors([]), [])
  })
})

describe("fetching", () => {
  const realFetch = globalThis.fetch
  const answer = (response: () => Response) => {
    globalThis.fetch = async () => response()
  }
  afterEach(() => {
    globalThis.fetch = realFetch
  })

  it("reads a repository with no commits (204 No Content) as nobody", async () => {
    answer(() => new Response(null, { status: 204 }))
    assert.deepEqual(await fetchRepoContributors("iii-hq/empty"), [])
  })

  it("falls back when a successful response is not JSON", async () => {
    answer(() => new Response("<html>", { status: 200 }))
    assert.equal(await fetchRepoContributors("iii-hq/iii"), null)
  })

  it("falls back when a successful response is not a list", async () => {
    answer(() => Response.json({ message: "unexpected" }))
    assert.equal(await fetchRepoContributors("iii-hq/iii"), null)
  })
})

describe("the published list", () => {
  const person = {
    id: 1,
    login: "ada",
    avatarUrl: "https://a/u/1?v=4",
    url: "https://github.com/ada",
    contributions: 3,
    repos: ["iii-hq/iii"],
  }

  it("accepts what generate-contributors writes", () => {
    assert.ok(isContributorsData({ repos: ["iii-hq/iii"], fetchedAt: "2026-10-29T03:00:00Z", contributors: [person] }))
  })

  it("rejects an empty list, a missing field and anything that is not an object", () => {
    assert.equal(isContributorsData({ repos: [], fetchedAt: "", contributors: [] }), false)
    assert.equal(isContributorsData({ repos: [], contributors: [{ ...person, login: undefined }] }), false)
    assert.equal(isContributorsData(null), false)
    assert.equal(isContributorsData("[]"), false)
  })
})
