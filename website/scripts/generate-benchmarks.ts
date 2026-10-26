// Writes content/benchmarks/data.json, the data /benchmarks renders, before every build and dev server
// (see package.json). CI publishes both sources to the gh-pages branch:
//
//   loadtest/index.json, loadtest/runs/<id>.json   load benchmarks (.github/workflows/loadtest.yml)
//   dev/bench/data.js                              criterion history (.github/workflows/bench-release.yml)
//
// Each part comes from the published branch when it can be fetched, and otherwise from the snapshot committed
// next to this data (content/benchmarks/snapshot.json), so a build never fails or ships an empty page because
// GitHub was slow. BENCHMARKS_OFFLINE=1 skips the network; BENCHMARKS_SOURCE points at another copy of the
// branch (a local checkout path or URL).
//
// Refreshing the snapshot from local files:
//   tsx scripts/generate-benchmarks.ts --snapshot --summary <summary.json> --micro <data.js>
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"

import type { BenchmarksData, IndexEntry, MicroData, MicroGroup, Release, Summary } from "../lib/benchmarks/types"
import { LOADTEST_SCHEMA } from "../lib/benchmarks/types"

const here = dirname(fileURLToPath(import.meta.url))
export const DATA = resolve(here, "../content/benchmarks/data.json")
export const SNAPSHOT = resolve(here, "../content/benchmarks/snapshot.json")
const DEFAULT_SOURCE = "https://raw.githubusercontent.com/iii-hq/iii/gh-pages"
/** The criterion history's series name (bench-release.yml `name:`). */
const MICRO_SERIES = "iii Engine Benchmarks"
/** Releases the micro-benchmark history keeps, newest last. */
const MICRO_RELEASES = 30

/** The release a criterion entry measured, from its commit ("… -- iii(iii/v0.24.6)"). */
export function releaseOf(message: string): string | null {
  return /iii\(iii\/v([^)\s]+)\)/.exec(message)?.[1] ?? null
}

interface CriterionEntry {
  commit: { id: string; message: string; timestamp?: string }
  date: number
  benches: { name: string; value: number; unit: string }[]
}

/**
 * The criterion history (`window.BENCHMARK_DATA = {…}`) as release-aligned series: the stable releases plus
 * the latest run (whatever it was), at most MICRO_RELEASES of them, grouped by the part of the name before
 * the first slash.
 */
export function parseMicro(js: string): MicroData {
  const json = JSON.parse(js.slice(js.indexOf("{")).trim().replace(/;\s*$/, "")) as {
    lastUpdate: number
    entries: Record<string, CriterionEntry[]>
  }
  const entries = [...(json.entries[MICRO_SERIES] ?? [])].sort((a, b) => a.date - b.date)
  const latest = entries.at(-1)
  const kept = entries.filter((e) => {
    const version = releaseOf(e.commit.message)
    return e === latest || (version !== null && !version.includes("-"))
  })
  // One entry per version: a re-run of a release replaces the earlier one.
  const byVersion = new Map<string, CriterionEntry>()
  for (const e of kept) byVersion.set(releaseOf(e.commit.message) ?? e.commit.id.slice(0, 7), e)
  const chosen = [...byVersion.values()].sort((a, b) => a.date - b.date).slice(-MICRO_RELEASES)

  const releases: Release[] = chosen.map((e) => ({
    version: releaseOf(e.commit.message) ?? e.commit.id.slice(0, 7),
    date: new Date(e.date).toISOString(),
    commit: e.commit.id,
  }))
  const names = new Map<string, string>()
  for (const e of chosen) for (const b of e.benches) names.set(b.name, b.unit)
  const groups = new Map<string, MicroGroup>()
  for (const [name, unit] of [...names].sort(([a], [b]) => a.localeCompare(b))) {
    const id = name.split("/")[0]
    const group = groups.get(id) ?? { id, benches: [] }
    group.benches.push({
      name,
      unit,
      values: chosen.map((e) => e.benches.find((b) => b.name === name)?.value ?? null),
    })
    groups.set(id, group)
  }
  return { updated: json.lastUpdate, releases, groups: [...groups.values()] }
}

/** The index entry a summary would have (the snapshot carries one run, without its published index). */
export function indexEntry(summary: Summary): IndexEntry {
  const { id, version, git_rev, date, machine, url, profiles, highlights } = summary
  return { id, version, git_rev, date, machine, url, path: `runs/${id}.json`, profiles, highlights }
}

async function fetchText(source: string, path: string): Promise<string> {
  if (!/^https?:\/\//.test(source)) return readFileSync(resolve(source, path), "utf8")
  const response = await fetch(`${source}/${path}`, { signal: AbortSignal.timeout(15_000) })
  if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`)
  return response.text()
}

async function publishedLoadtest(source: string): Promise<BenchmarksData["loadtest"]> {
  const index = JSON.parse(await fetchText(source, "loadtest/index.json")) as { schema: number; runs: IndexEntry[] }
  const runs = index.runs.filter((r) => r.highlights)
  const newest = runs[0]
  if (!newest) throw new Error("loadtest/index.json lists no run")
  const latest = JSON.parse(await fetchText(source, `loadtest/${newest.path}`)) as Summary
  if (latest.schema !== LOADTEST_SCHEMA) throw new Error(`run ${latest.id}: schema ${latest.schema}`)
  return { index: runs, latest }
}

function readSnapshot(): BenchmarksData | null {
  return existsSync(SNAPSHOT) ? (JSON.parse(readFileSync(SNAPSHOT, "utf8")) as BenchmarksData) : null
}

export async function generate(env: NodeJS.ProcessEnv = process.env): Promise<BenchmarksData> {
  const snapshot = readSnapshot()
  const source = env.BENCHMARKS_SOURCE ?? DEFAULT_SOURCE
  const offline = Boolean(env.BENCHMARKS_OFFLINE)
  const data: BenchmarksData = {
    generatedAt: new Date().toISOString(),
    sources: { loadtest: "none", micro: "none" },
    loadtest: null,
    micro: null,
  }

  try {
    if (offline) throw new Error("BENCHMARKS_OFFLINE is set")
    data.loadtest = await publishedLoadtest(source)
    data.sources.loadtest = "published"
  } catch (err) {
    console.warn(`generate-benchmarks: load benchmarks from the snapshot (${(err as Error).message})`)
    data.loadtest = snapshot?.loadtest ?? null
    data.sources.loadtest = data.loadtest ? "snapshot" : "none"
  }

  try {
    if (offline) throw new Error("BENCHMARKS_OFFLINE is set")
    data.micro = parseMicro(await fetchText(source, "dev/bench/data.js"))
    data.sources.micro = "published"
  } catch (err) {
    console.warn(`generate-benchmarks: micro-benchmarks from the snapshot (${(err as Error).message})`)
    data.micro = snapshot?.micro ?? null
    data.sources.micro = data.micro ? "snapshot" : "none"
  }
  return data
}

function write(path: string, data: BenchmarksData) {
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, `${JSON.stringify(data)}\n`)
}

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(name)
  return i === -1 ? undefined : process.argv[i + 1]
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (isMain) {
  if (process.argv.includes("--snapshot")) {
    const summaryPath = arg("--summary")
    const microPath = arg("--micro")
    if (!summaryPath || !microPath) throw new Error("--snapshot needs --summary <summary.json> --micro <data.js>")
    const latest = JSON.parse(readFileSync(summaryPath, "utf8")) as Summary
    write(SNAPSHOT, {
      generatedAt: new Date().toISOString(),
      sources: { loadtest: "snapshot", micro: "snapshot" },
      loadtest: { index: [indexEntry(latest)], latest },
      micro: parseMicro(readFileSync(microPath, "utf8")),
    })
    console.log(`generate-benchmarks: wrote ${SNAPSHOT}`)
  } else {
    const data = await generate()
    write(DATA, data)
    console.log(
      `generate-benchmarks: load benchmarks ${data.sources.loadtest}, micro-benchmarks ${data.sources.micro} → ${DATA}`,
    )
  }
}
