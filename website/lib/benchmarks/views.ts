/**
 * What each chart on /benchmarks draws, taken from one load-test run. Pure functions over the published
 * summary, so they run at build time and are tested on their own (scripts/generate-benchmarks.test.ts).
 */

import { compact, joined, micros, nanos, percent, profileLabel, profileShort, rate } from './format'
import type { Highlights, IndexEntry, MicroData, ProfileInfo, Run, Step, Summary } from './types'

export interface Point {
  x: number
  y: number
  /** Extra lines for the tooltip, under the value. */
  detail?: string[]
}

export interface Series {
  /** The profile's name (`2x1024`): what the page's profile selection matches. */
  id: string
  label: string
  points: Point[]
  /** Why the line stops early (the engine ran out of memory, say). */
  note?: string
}

export interface ProfileView extends ProfileInfo {
  label: string
  short: string
}

/** VMs smallest first, the host last: the order every chart and the switcher use. */
export function profiles(summary: Summary): ProfileView[] {
  return [...summary.profiles]
    .sort((a, b) => (a.kind === b.kind ? a.cpus - b.cpus : a.kind === 'host' ? 1 : -1))
    .map((p) => ({ ...p, label: profileLabel(p), short: profileShort(p) }))
}

function run(summary: Summary, profile: string, scenario: string): Run | undefined {
  return summary.runs.find((r) => r.profile === profile && r.scenario === scenario)
}

const num = (v: unknown): number | undefined => (typeof v === 'number' && Number.isFinite(v) ? v : undefined)

/** Why a scenario stopped on a profile, in a few words, or nothing when it finished. */
export function stopNote(r: Run | undefined): string | undefined {
  if (!r || r.ok) return undefined
  // The line ends on the last step measured; the engine ran out on the one after it.
  if (r.oom_killed) return `Out of memory past here (${r.engine_mem_cap_mb ?? '?'} MiB cap)`
  if (r.error?.includes('timed out')) return 'Stopped answering'
  return 'Did not finish'
}

/** The steps a run measured (not the one its engine died in). */
const measured = (r: Run) => r.steps.filter((s) => s.unit !== 'failed')

/**
 * Which steps a chart draws a size's line through, so a failure is noted on the line it cut short and not on
 * another chart of the same scenario. A run records the step its engine died in; for runs from before that,
 * `fallback` decides.
 */
interface Path {
  onPath: (s: Step) => boolean
  fallback?: (r: Run) => boolean
}

function noteFor(r: Run, path: Path): string | undefined {
  const note = stopNote(r)
  if (!note) return undefined
  const died = r.steps.find((s) => s.unit === 'failed')
  if (died) return path.onPath(died) ? note : undefined
  return (path.fallback?.(r) ?? true) ? note : undefined
}

const anyStep: Path = { onPath: () => true }

function seriesFor(
  summary: Summary,
  scenario: string,
  pick: (s: Step) => Point | undefined,
  path: Path = anyStep,
): Series[] {
  return profiles(summary).flatMap((p) => {
    const r = run(summary, p.name, scenario)
    if (!r) return []
    const points = measured(r)
      .map(pick)
      .filter((pt): pt is Point => pt !== undefined)
    if (points.length === 0 && r.ok) return []
    return [{ id: p.name, label: p.short, points, note: noteFor(r, path) }]
  })
}

const cpuLine = (s: Step, cpus: number) => `engine ${s.engine_cpu.toFixed(2)} of ${cpus} CPU${cpus === 1 ? '' : 's'}`

const routingPath: Path = { onPath: (s) => num(s.params.payload_bytes) === 64 }

/** `rpc`: calls per second against concurrency (the 64-byte payload steps). */
export function routing(summary: Summary): Series[] {
  return profiles(summary).flatMap((p) => {
    const r = run(summary, p.name, 'rpc')
    if (!r) return []
    const points = measured(r)
      .filter((s) => num(s.params.payload_bytes) === 64)
      .map((s) => ({
        x: num(s.params.concurrency) ?? 0,
        y: s.rate,
        detail: [`p50 ${joined(micros(s.p50_us ?? 0))} · p99 ${joined(micros(s.p99_us ?? 0))}`, cpuLine(s, p.cpus)],
      }))
    return [{ id: p.name, label: p.short, points, note: noteFor(r, routingPath) }]
  })
}

/** `rpc`: p99 latency against concurrency. */
export function routingLatency(summary: Summary): Series[] {
  return profiles(summary).flatMap((p) => {
    const r = run(summary, p.name, 'rpc')
    if (!r) return []
    const points = measured(r)
      .filter((s) => num(s.params.payload_bytes) === 64 && s.p99_us !== null)
      .map((s) => ({
        x: num(s.params.concurrency) ?? 0,
        y: s.p99_us ?? 0,
        detail: [`p50 ${joined(micros(s.p50_us ?? 0))}`, `${joined(rate(s.rate))} calls`],
      }))
    return [{ id: p.name, label: p.short, points, note: noteFor(r, routingPath) }]
  })
}

/** `fanout`: activations per second against observers per function (from one observer up). */
export function fanout(summary: Summary): Series[] {
  return seriesFor(
    summary,
    'fanout',
    (s) => {
      const k = num(s.params.observers_per_function)
      const activations = num(s.extra.activations_per_sec)
      if (!k || activations === undefined) return undefined
      const expected = num(s.extra.activations_expected)
      const delivered = num(s.extra.activations_delivered)
      return {
        x: k,
        y: activations,
        detail: [
          `${joined(rate(s.rate))} calls, p99 ${joined(micros(s.p99_us ?? 0))}`,
          expected && delivered !== undefined ? `${percent(delivered / expected)} of activations delivered` : '',
        ],
      }
    },
    { onPath: (s) => num(s.params.observers_per_function) !== undefined },
  )
}

/** `fanout`: the observed calls themselves, against observers per function. */
export function fanoutCalls(summary: Summary): Series[] {
  return seriesFor(
    summary,
    'fanout',
    (s) => {
      const k = num(s.params.observers_per_function)
      if (k === undefined || s.unit !== 'calls') return undefined
      return { x: k, y: s.rate, detail: [`p99 ${joined(micros(s.p99_us ?? 0))}`] }
    },
    { onPath: (s) => num(s.params.observers_per_function) !== undefined },
  )
}

/**
 * The paced ladder's path. A run from before failed steps were recorded died off it when the ladder had already
 * reached a rate it could not deliver in full: the ladder stops there, so the engine died in a later step.
 */
const deliveryPath: Path = {
  onPath: (s) => num(s.params.offered_per_sec) !== undefined,
  fallback: (r) => !measured(r).some((s) => (num(s.extra.delivery_ratio) ?? 1) < 0.95),
}

/** `emit` (paced ladder): activations delivered per second against what the owner offered. */
export function delivery(summary: Summary): Series[] {
  return seriesFor(
    summary,
    'emit',
    (s) => {
      const offered = num(s.extra.offered_per_sec)
      const ratio = num(s.extra.delivery_ratio)
      const delivered = num(s.extra.activations_per_sec)
      if (offered === undefined || ratio === undefined || delivered === undefined) return undefined
      return {
        x: offered,
        y: delivered,
        detail: [`${joined(rate(offered))} offered`, `${percent(ratio)} delivered`],
      }
    },
    deliveryPath,
  )
}

/** `filters`: calls per second against observers with a filter each (every call matches one). */
export function filters(summary: Summary): Series[] {
  return profiles(summary).flatMap((p) => {
    const r = run(summary, p.name, 'filters')
    if (!r) return []
    const points = measured(r).flatMap((s) => {
      const n = num(s.params.observers)
      if (n === undefined) return []
      return [{ x: n, y: s.rate, detail: [`p99 ${joined(micros(s.p99_us ?? 0))}`, cpuLine(s, p.cpus)] }]
    })
    return [
      {
        id: p.name,
        label: p.short,
        points,
        note: noteFor(r, { onPath: (s) => num(s.params.observers) !== undefined }),
      },
    ]
  })
}

/** A step at rest: what the engine holds once nothing is moving. */
const atRest = (s: Step) => s.unit === 'idle'

/** `observers`: engine memory against observers held, at rest. */
export function observerMemory(summary: Summary): Series[] {
  return seriesFor(summary, 'observers', (s) => {
    const n = num(s.params.observers)
    if (!atRest(s) || n === undefined || s.label.startsWith('after')) return undefined
    const per = num(s.extra.rss_bytes_per_observer)
    return { x: n, y: s.rss_avg_mb, detail: per ? [`${joined(compact(per))}B per observer`] : undefined }
  })
}

/** `connections`: engine memory against idle workers connected. */
export function workerMemory(summary: Summary): Series[] {
  return seriesFor(summary, 'connections', (s) => {
    const n = num(s.params.workers)
    if (!atRest(s) || n === undefined || s.label === 'just closed' || s.label.startsWith('after')) return undefined
    const per = num(s.extra.rss_bytes_per_worker)
    return { x: n, y: s.rss_avg_mb, detail: per ? [`${joined(compact(per))}B per worker`] : undefined }
  })
}

/** `overload`: the engine's peak memory against slow observers with full queues (concurrency 64). */
export function overload(summary: Summary): Series[] {
  return seriesFor(
    summary,
    'overload',
    (s) => {
      const n = num(s.params.observers)
      if (atRest(s) || n === undefined || num(s.params.concurrency) !== 64) return undefined
      const dropped = s.extra.engine as { dropped?: Record<string, number> } | undefined
      const overflow = dropped?.dropped?.OVERFLOW
      return {
        x: n,
        y: s.rss_max_mb,
        detail: [
          `${joined(rate(s.rate))} calls`,
          overflow ? `${compact(overflow).value}${compact(overflow).unit} activations dropped` : 'nothing dropped',
        ],
      }
    },
    { onPath: (s) => num(s.params.concurrency) === 64 },
  )
}

export interface LocalRow {
  profile: ProfileView
  cases: { label: string; rate: number; engineCpu: number; activations: number | null }[]
}

/** `local`: in-process calls, unobserved and observed, with what the engine spent. */
export function local(summary: Summary): LocalRow[] {
  return profiles(summary).flatMap((p) => {
    const r = run(summary, p.name, 'local')
    if (!r) return []
    const cases = measured(r)
      .filter((s) => !atRest(s))
      .map((s) => ({
        label: s.label,
        rate: s.rate,
        engineCpu: s.engine_cpu,
        activations: num(s.extra.activations_per_sec) ?? null,
      }))
    return cases.length ? [{ profile: p, cases }] : []
  })
}

export interface MicroRow {
  name: string
  /** The latest time, formatted. */
  latest: string
  /** Change against the release before, as a ratio (0.1 = 10% slower), when both exist. */
  change: number | null
  /** Every release's time, oldest first (releases without it left out), for the sparkline... */
  history: number[]
  /** ...and the release each was measured at. */
  releases: string[]
}

/** A micro-benchmark group's rows, latest release against the one before. Benchmarks retired before the
 * latest release are left out. */
export function microRows(micro: MicroData, group: string): MicroRow[] {
  const g = micro.groups.find((x) => x.id === group)
  if (!g) return []
  const benches = [...g.benches].sort((a, b) => order(a.name, b.name))
  return benches.flatMap((b) => {
    if (b.values.at(-1) === null) return []
    const present = b.values.flatMap((v, i) => (v === null ? [] : [{ v, release: micro.releases[i].version }]))
    const latest = present.at(-1)?.v ?? 0
    const before = present.at(-2)?.v
    return [
      {
        name: b.name.slice(group.length + 1) || b.name,
        latest: joined(nanos(latest)),
        change: before ? latest / before - 1 : null,
        history: present.map((p) => p.v),
        releases: present.map((p) => p.release),
      },
    ]
  })
}

const sizes: Record<string, number> = { b: 1, kb: 1024, mb: 1024 ** 2, gb: 1024 ** 3 }

/** Benchmark names in reading order: 8 before 32, 10kb before 1mb, the rest alphabetically. */
function order(a: string, b: string): number {
  const size = (name: string) => {
    const m = /(\d+(?:\.\d+)?)(b|kb|mb|gb)$/i.exec(name)
    return m ? Number(m[1]) * sizes[m[2].toLowerCase()] : null
  }
  const [sa, sb] = [size(a), size(b)]
  if (sa !== null && sb !== null && a.replace(/[\d.]+\w*$/, '') === b.replace(/[\d.]+\w*$/, '')) return sa - sb
  return a.localeCompare(b, 'en', { numeric: true })
}

/** One headline number across published runs, oldest first: x is the run's position, labelled by its version. */
export function history(
  index: IndexEntry[],
  pick: (h: Highlights) => number | null,
): { series: Series[]; labels: string[] } {
  const runs = [...index].reverse()
  const labels = runs.map((r) => r.version)
  const names = new Map<string, ProfileView>()
  for (const r of runs)
    for (const p of r.profiles) names.set(p.name, { ...p, label: profileLabel(p), short: profileShort(p) })
  const ordered = [...names.values()].sort((a, b) => (a.kind === b.kind ? a.cpus - b.cpus : a.kind === 'host' ? 1 : -1))
  const series = ordered.flatMap((p) => {
    const points = runs.flatMap((r, i) => {
      const h = r.highlights[p.name]
      const v = h ? pick(h) : null
      return v === null || v === undefined ? [] : [{ x: i, y: v, detail: [r.date.slice(0, 10)] }]
    })
    return points.length ? [{ id: p.name, label: p.short, points }] : []
  })
  return { series, labels }
}
