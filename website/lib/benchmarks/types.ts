/**
 * The data behind /benchmarks. Two sources, both published to the gh-pages branch by CI and read when the site
 * builds (scripts/generate-benchmarks.ts):
 *
 * - Load benchmarks (crates/iii-loadtest, .github/workflows/loadtest.yml): the engine alone in microVMs of
 *   fixed size, driven by real SDK workers. `loadtest/index.json` lists every run with its highlights;
 *   `loadtest/runs/<id>.json` is one run in full. The shapes mirror `crates/iii-loadtest/src/export.rs`.
 * - Micro-benchmarks (engine/benches, .github/workflows/bench-release.yml): criterion timings of single code
 *   paths, one entry per release in `dev/bench/data.js`.
 */

/** The `export.rs` schema this page draws. A run of another schema is left out rather than drawn wrong. */
export const LOADTEST_SCHEMA = 1

export interface Machine {
  label: string | null
  cpu: string
  cpus: number
  mem_mb: number
  load_vcpus: number
  load_ram_mb: number
}

export interface ProfileInfo {
  /** `host`, or `<vcpus>x<MiB>` for a microVM. */
  name: string
  kind: 'vm' | 'host'
  /** The engine's CPUs: a VM's vCPUs, or the host CPUs it was pinned to. */
  cpus: number
  /** The engine's memory cap (a VM's); none on the host. */
  mem_mb: number | null
}

export interface Failure {
  scenario: string
  oom_killed: boolean
  error: string
}

/** The handful of numbers a profile is summed up by; `null` when the scenario did not measure it. */
export interface Highlights {
  calls_per_sec: number | null
  calls_p99_us: number | null
  calls_concurrency: number | null
  cpu_us_per_call: number | null
  fanout_activations_per_sec: number | null
  sustained_activations_per_sec: number | null
  saturated_at: number | null
  saturated_delivered: number | null
  filtered_calls_per_sec: number | null
  filtered_observers: number | null
  bytes_per_observer: number | null
  observers_held: number | null
  bytes_per_worker: number | null
  workers_held: number | null
  local_calls_per_sec: number | null
  local_engine_cpu: number | null
  idle_rss_mb: number | null
  overload_rss_mb: number | null
  overload_observers: number | null
  failed: Failure[]
}

export interface Step {
  label: string
  params: Record<string, unknown>
  unit: string
  /** `unit` per second; 0 for a step at rest. */
  rate: number
  errors: number
  p50_us: number | null
  p99_us: number | null
  /** Cores the engine process used (1 = one CPU busy). */
  engine_cpu: number
  /** Cores of the engine's CPUs that were busy, kernel work on them included. */
  cpus_busy: number
  rss_avg_mb: number
  rss_max_mb: number
  loadgen_cpu: number
  extra: Record<string, unknown>
}

export interface Run {
  profile: string
  scenario: string
  ok: boolean
  error: string | null
  oom_killed: boolean
  engine_cpus: number
  engine_mem_cap_mb: number | null
  steps: Step[]
}

export interface Summary {
  schema: number
  id: string
  version: string
  git_rev: string | null
  /** RFC 3339, UTC. */
  date: string
  machine: Machine
  url: string | null
  settings: { step_secs?: number; observability?: string; engine_log_level?: string; observers?: string[] }
  profiles: ProfileInfo[]
  highlights: Record<string, Highlights>
  runs: Run[]
}

export interface IndexEntry {
  id: string
  version: string
  git_rev: string | null
  date: string
  machine: Machine
  url: string | null
  path: string
  profiles: ProfileInfo[]
  highlights: Record<string, Highlights>
}

/** One micro-benchmark across releases. `values[i]` is its time at `releases[i]`, `null` before it existed. */
export interface MicroBench {
  name: string
  unit: string
  values: (number | null)[]
}

export interface MicroGroup {
  id: string
  benches: MicroBench[]
}

export interface Release {
  version: string
  date: string
  commit: string
}

export interface MicroData {
  /** Epoch milliseconds of the last criterion run. */
  updated: number
  releases: Release[]
  groups: MicroGroup[]
}

/** What the page renders: written by scripts/generate-benchmarks.ts into content/benchmarks/data.json. */
export interface BenchmarksData {
  generatedAt: string
  /** Where each part came from: the published branch, or the snapshot committed with the site. */
  sources: { loadtest: 'published' | 'snapshot' | 'none'; micro: 'published' | 'snapshot' | 'none' }
  loadtest: { index: IndexEntry[]; latest: Summary } | null
  micro: MicroData | null
}
