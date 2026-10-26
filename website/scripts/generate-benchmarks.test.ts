import assert from "node:assert/strict"
import { describe, it } from "node:test"

import { compact, profileLabel } from "../lib/benchmarks/format"
import type { Highlights, Summary } from "../lib/benchmarks/types"
import {
  delivery,
  history,
  microRows,
  observerMemory,
  overload,
  profiles,
  routing,
  stopNote,
} from "../lib/benchmarks/views"
import { indexEntry, parseMicro, releaseOf } from "./generate-benchmarks"

const entry = (message: string, date: number, benches: [string, number][]) => ({
  commit: { id: `${date}abcdef0`, message },
  date,
  tool: "cargo",
  benches: benches.map(([name, value]) => ({ name, value, range: "± 1", unit: "ns/iter" })),
})

const criterion = (entries: unknown[]) =>
  `window.BENCHMARK_DATA = ${JSON.stringify({ lastUpdate: 99, repoUrl: "x", entries: { "iii Engine Benchmarks": entries } })}`

describe("micro-benchmark history", () => {
  it("reads the release from a release commit", () => {
    assert.equal(releaseOf("chore: bump versions for release -- iii(iii/v0.24.6)"), "0.24.6")
    assert.equal(releaseOf("chore: bump -- iii(iii/v0.25.0-rc.1)\n\nbody"), "0.25.0-rc.1")
    assert.equal(releaseOf("fix: something"), null)
  })

  it("keeps stable releases and the latest run, aligned per benchmark", () => {
    const micro = parseMicro(
      criterion([
        entry("release -- iii(iii/v0.24.5)", 1, [["ws_roundtrip/invoke_echo", 150]]),
        entry("release -- iii(iii/v0.24.6-rc.1)", 2, [["ws_roundtrip/invoke_echo", 140]]),
        entry("release -- iii(iii/v0.24.6)", 3, [
          ["ws_roundtrip/invoke_echo", 130],
          ["observe_dispatch/auto/1", 3600],
        ]),
        entry("release -- iii(iii/v0.25.0-rc.1)", 4, [["ws_roundtrip/invoke_echo", 120]]),
      ]),
    )
    // The rc in the middle is gone; the latest run stays though it is an rc.
    assert.deepEqual(
      micro.releases.map((r) => r.version),
      ["0.24.5", "0.24.6", "0.25.0-rc.1"],
    )
    const ws = micro.groups.find((g) => g.id === "ws_roundtrip")
    assert.deepEqual(ws?.benches[0].values, [150, 130, 120])
    const observe = micro.groups.find((g) => g.id === "observe_dispatch")
    assert.deepEqual(observe?.benches[0].values, [null, 3600, null])
    assert.equal(micro.updated, 99)
  })

  it("orders rows for reading and leaves retired benchmarks out", () => {
    const micro = parseMicro(
      criterion([
        entry("release -- iii(iii/v1.0.0)", 1, [
          ["sizes/1mb", 9],
          ["sizes/10kb", 3],
          ["sizes/1kb", 1],
          ["fan/128", 4],
          ["fan/8", 2],
          ["fan/old", 7],
        ]),
        entry("release -- iii(iii/v1.0.1)", 2, [
          ["sizes/1mb", 10],
          ["sizes/10kb", 3],
          ["sizes/1kb", 1],
          ["fan/128", 5],
          ["fan/8", 2],
        ]),
      ]),
    )
    assert.deepEqual(
      microRows(micro, "sizes").map((r) => r.name),
      ["1kb", "10kb", "1mb"],
    )
    const fan = microRows(micro, "fan")
    assert.deepEqual(
      fan.map((r) => r.name),
      ["8", "128"],
    )
    assert.equal(fan[1].change, 0.25)
    assert.deepEqual(fan[1].releases, ["1.0.0", "1.0.1"])
  })
})

const step = (params: Record<string, unknown>, rate: number, extra: Record<string, unknown> = {}, rss = 50) => ({
  label: "step",
  params,
  unit: "calls",
  rate,
  errors: 0,
  p50_us: 100,
  p99_us: 900,
  engine_cpu: 1.5,
  cpus_busy: 1.6,
  rss_avg_mb: rss,
  rss_max_mb: rss,
  loadgen_cpu: 2,
  extra,
})

const summary: Summary = {
  schema: 1,
  id: "0.25.0-x",
  version: "0.25.0",
  git_rev: "abc",
  date: "2026-10-19T20:04:05Z",
  machine: { label: null, cpu: "EPYC", cpus: 16, mem_mb: 64000, load_vcpus: 6, load_ram_mb: 2048 },
  url: null,
  settings: {},
  profiles: [
    { name: "host", kind: "host", cpus: 8, mem_mb: null },
    { name: "2x1024", kind: "vm", cpus: 2, mem_mb: 1024 },
  ],
  highlights: {},
  runs: [
    {
      profile: "2x1024",
      scenario: "rpc",
      ok: true,
      error: null,
      oom_killed: false,
      engine_cpus: 2,
      engine_mem_cap_mb: 1024,
      steps: [
        step({ concurrency: 1, payload_bytes: 64 }, 4000),
        step({ concurrency: 32, payload_bytes: 64 }, 72000),
        step({ concurrency: 32, payload_bytes: 16384 }, 35000),
      ],
    },
    {
      profile: "2x1024",
      scenario: "emit",
      ok: true,
      error: null,
      oom_killed: false,
      engine_cpus: 2,
      engine_mem_cap_mb: 1024,
      steps: [
        step({ observers: 16 }, 3_000_000),
        step({ observers: 16 }, 1000, { offered_per_sec: 1000, delivery_ratio: 1, activations_per_sec: 999 }),
      ],
    },
    {
      profile: "2x1024",
      scenario: "observers",
      ok: false,
      error: "the engine was OOM-killed at its 1024 MiB cap (the engine exited during the step)",
      oom_killed: true,
      engine_cpus: 2,
      engine_mem_cap_mb: 1024,
      steps: [
        { ...step({ observers: 0 }, 0, {}, 41), unit: "idle", label: "0 observers (at rest)" },
        {
          ...step({ observers: 1000 }, 0, { rss_bytes_per_observer: 9000 }, 50),
          unit: "idle",
          label: "1000 (at rest)",
        },
        { ...step({ observers: 4000, created: 3000 }, 9000), unit: "observers", label: "create → 4000" },
      ],
    },
  ],
}

describe("chart views", () => {
  it("orders sizes smallest first, the host last, and names them", () => {
    assert.deepEqual(
      profiles(summary).map((p) => [p.name, p.label, p.short]),
      [
        ["2x1024", "2 vCPU · 1 GiB", "2 vCPU"],
        ["host", "Host · 8 CPUs", "Host"],
      ],
    )
    assert.equal(profileLabel({ kind: "vm", cpus: 1, mem_mb: 512 }), "1 vCPU · 512 MiB")
  })

  it("draws routing from the 64-byte steps only", () => {
    const [vm] = routing(summary)
    assert.equal(vm.id, "2x1024")
    assert.deepEqual(
      vm.points.map((p) => [p.x, p.y]),
      [
        [1, 4000],
        [32, 72000],
      ],
    )
  })

  it("draws delivery from the paced steps, not the flood", () => {
    const [vm] = delivery(summary)
    assert.deepEqual(
      vm.points.map((p) => [p.x, p.y]),
      [[1000, 999]],
    )
  })

  it("keeps memory at rest only, and says why a line stops", () => {
    const [vm] = observerMemory(summary)
    assert.deepEqual(
      vm.points.map((p) => [p.x, p.y]),
      [
        [0, 41],
        [1000, 50],
      ],
    )
    assert.equal(vm.note, "Out of memory past here (1024 MiB cap)")
    assert.equal(stopNote(undefined), undefined)
  })

  it("notes a failure only on the chart whose steps it cut short", () => {
    const failedRun = (scenario: string, steps: ReturnType<typeof step>[]) => ({
      profile: "2x1024",
      scenario,
      ok: false,
      error: "OOM",
      oom_killed: true,
      engine_cpus: 2,
      engine_mem_cap_mb: 1024,
      steps,
    })
    const died = (params: Record<string, unknown>) => ({ ...step(params, 0), unit: "failed", label: "died" })
    const paced = (offered: number, ratio: number) =>
      step({ observers: 16, offered_per_sec: offered }, offered, {
        offered_per_sec: offered,
        delivery_ratio: ratio,
        activations_per_sec: offered * ratio,
      })
    const withRuns = (runs: ReturnType<typeof failedRun>[]) => ({ ...summary, runs })

    // Died in the 256-observer flood after the ladder: the delivery line is whole.
    const flood = withRuns([failedRun("emit", [paced(1000, 1), died({ observers: 256, emitters: 4 })])])
    assert.equal(delivery(flood)[0].note, undefined)
    assert.deepEqual(
      delivery(flood)[0].points.map((p) => p.x),
      [1000],
    )
    // Died on a paced step: noted.
    const onLadder = withRuns([failedRun("emit", [paced(1000, 1), died({ observers: 16, offered_per_sec: 5000 })])])
    assert.equal(delivery(onLadder)[0].note, "Out of memory past here (1024 MiB cap)")
    // An older run with no failed step: the ladder had reached a rate it could not deliver, so it died later.
    const older = withRuns([failedRun("emit", [paced(1000, 1), paced(50000, 0.7)])])
    assert.equal(delivery(older)[0].note, undefined)
    const olderCut = withRuns([failedRun("emit", [paced(1000, 1)])])
    assert.equal(delivery(olderCut)[0].note, "Out of memory past here (1024 MiB cap)")
    // Overload draws the concurrency-64 steps: dying at concurrency 256 is not on its path.
    const overloaded = withRuns([
      failedRun("overload", [step({ observers: 4, concurrency: 64 }, 1000), died({ observers: 4, concurrency: 256 })]),
    ])
    assert.equal(overload(overloaded)[0].note, undefined)
  })

  it("files a summary in the index with its path", () => {
    const e = indexEntry(summary)
    assert.equal(e.path, "runs/0.25.0-x.json")
    assert.equal(e.version, "0.25.0")
  })

  it("draws history oldest first, a line per size, skipping runs without the number", () => {
    const h = (calls: number | null) => ({ calls_per_sec: calls, failed: [] }) as unknown as Highlights
    const run = (version: string, date: string, calls: Record<string, number | null>) => ({
      ...indexEntry(summary),
      id: version,
      version,
      date,
      highlights: Object.fromEntries(Object.entries(calls).map(([k, v]) => [k, h(v)])),
    })
    const { series, labels } = history(
      [
        run("0.26.0", "2026-12-01T00:00:00Z", { "2x1024": 80000, host: 500000 }),
        run("0.25.0", "2026-10-19T00:00:00Z", { "2x1024": 72000, host: null }),
      ],
      (x) => x.calls_per_sec,
    )
    assert.deepEqual(labels, ["0.25.0", "0.26.0"])
    assert.deepEqual(
      series.map((s) => [s.id, s.points.map((p) => [p.x, p.y])]),
      [
        [
          "2x1024",
          [
            [0, 72000],
            [1, 80000],
          ],
        ],
        ["host", [[1, 500000]]],
      ],
    )
  })

  it("formats compactly without a trailing zero", () => {
    assert.deepEqual(compact(1000), { value: "1", unit: "k" })
    assert.deepEqual(compact(4200), { value: "4.2", unit: "k" })
    assert.deepEqual(compact(0), { value: "0", unit: "" })
    assert.deepEqual(compact(1_500_000), { value: "1.5", unit: "M" })
  })
})
