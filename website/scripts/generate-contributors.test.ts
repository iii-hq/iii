import assert from "node:assert/strict"
import { describe, it } from "node:test"

import { CONTRIBUTOR_REPOS, type GitHubContributor, isBot, mergeContributors, repoSlug } from "../lib/contributors"

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
