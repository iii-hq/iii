/* tracks — the two-track split (A9 toggle). data only. */

export type TrackId = 'integration' | 'e2e'

export interface TrackProfile {
  id: TrackId
  label: string
  headline: string
  rows: Array<{ k: string; v: string; detail: string }>
}

export const TRACKS: TrackProfile[] = [
  {
    id: 'integration',
    label: 'integration tests',
    headline: 'prove the public contracts, deterministically.',
    rows: [
      {
        k: 'v1 gate',
        v: 'C-E2E-001 + C-E2E-002',
        detail: 'streamed text reaches durable completion; an allowed function executes exactly once. quarantined reproductions (C-E2E-505/506/507) are checked in beside them but stay outside the gate until fixed.',
      },
      {
        k: 'model boundary',
        v: 'scripted router::* worker',
        detail: 'the real llm-router and providers are absent; a strict script owns the six fixed router functions.',
      },
      {
        k: 'primary oracle',
        v: 'code assertions',
        detail: 'over public, durable evidence: send response, status, full transcript, recorder log, lifecycle events.',
      },
      {
        k: 'execution owner',
        v: 'standalone rust runner',
        detail: 'harness/evals/integration owns process supervision, fixtures, evidence, grading, and reports.',
      },
      {
        k: 'v1 execution',
        v: 'pull-request regression',
        detail: 'one fresh isolated stack per scenario, run serially; ci runs each scenario twice and requires byte-identical result.json, with no model key anywhere.',
      },
      {
        k: 'release policy',
        v: 'earned promotion',
        detail: 'required-check status only after 100 consecutive clean runs across 14 days, zero skips, zero unexplained flakes.',
      },
    ],
  },
  {
    id: 'e2e',
    label: 'e2e tests',
    headline: 'measure whether real workflows succeed.',
    rows: [
      {
        k: 'v1 gate',
        v: 'four real-model tests',
        detail: 'plain response, single function, sub-agent fan-out/fan-in, and triggered work — all passing end-to-end, headless, through public boundaries only.',
      },
      {
        k: 'model boundary',
        v: 'production router + pinned model',
        detail: 'the same harness, router, provider, and function boundaries production uses. a pinned model is an immutable provider version, never a floating alias.',
      },
      {
        k: 'primary oracle',
        v: 'explicit expect() assertions',
        detail: 'over durable records and fixture state, plus the versioned evidence reads: harness::session-tree, harness::metrics, harness::triggered-work. no model grader in the suite.',
      },
      {
        k: 'execution owner',
        v: 'vitest + @iii/harness-test',
        detail: 'ordinary test files calling worker.trigger on public functions; the shared harness-test worker holds only the lifecycle sink and test-scoped evidence capture.',
      },
      {
        k: 'v1 execution',
        v: 'scheduled real-model runs',
        detail: 'headless, against a dedicated stack with scoped provider keys; scheduled and on-demand, not a required pull-request gate.',
      },
      {
        k: 'release policy',
        v: 'no aggregate score',
        detail: 'version 1 computes no composite quality score: raw metrics only, run on a schedule to characterize noise before any threshold gates a release.',
      },
    ],
  },
]

export const ADJACENT_SYSTEMS = [
  {
    name: 'HarnessBench',
    type: 'outside this spec',
    desc: 'same-prompt performance comparison (pr #280) with its own run record and ui. it defines no correctness assertions, multi-turn scenarios, or release gates.',
  },
  {
    name: 'workflow worker',
    type: 'outside this spec',
    desc: 'dag execution. it may sit in the subject worker set, but the e2e suite does not modify its dag or retry model.',
  },
  {
    name: 'harness::react',
    type: 'not an evaluator',
    desc: 'maps events to agent turns; it exposes none of the evidence contracts or assertions this suite requires.',
  },
] as const
