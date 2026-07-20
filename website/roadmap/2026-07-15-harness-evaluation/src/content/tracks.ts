/* tracks — the two-track split (A9 toggle). data only. */

export type TrackId = 'integration' | 'quality'

export interface TrackProfile {
  id: TrackId
  label: string
  headline: string
  rows: Array<{ k: string; v: string; detail: string }>
}

export const TRACKS: TrackProfile[] = [
  {
    id: 'integration',
    label: 'integration',
    headline: 'prove the public contracts, deterministically.',
    rows: [
      {
        k: 'status',
        v: 'core implemented',
        detail: 'the runner, live-contract readiness, scenario compiler, and typed teardown exist. phase 1 scenarios and remaining isolation/port-collision coverage remain.',
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
        k: 'first use',
        v: 'pull-request regression',
        detail: 'one fresh isolated stack per scenario, run serially, reproducible without a model key.',
      },
      {
        k: 'release policy',
        v: 'earned promotion',
        detail: 'required-check status only after 100 consecutive clean runs across 14 days, zero skips, zero unexplained flakes.',
      },
    ],
  },
  {
    id: 'quality',
    label: 'agent quality',
    headline: 'measure whether real workflows succeed.',
    rows: [
      {
        k: 'status',
        v: 'proposed · revised',
        detail: 'revised 2026-07-20 after design review: test cases are code, not manifests. delivery starts after the harness::session-tree and harness::metrics contracts exist.',
      },
      {
        k: 'model boundary',
        v: 'production router + pinned model',
        detail: 'the same harness, router, provider, and function boundaries used in production, with pinned inputs.',
      },
      {
        k: 'primary oracle',
        v: 'explicit expect() assertions',
        detail: 'plain code assertions over durable records and fixture state, plus raw metrics from assets the harness builds by default. no model grader in the suite.',
      },
      {
        k: 'execution owner',
        v: 'vitest + harness-test worker',
        detail: 'an ordinary e2e suite calling trigger() on public functions directly; the shared harness-test worker holds only helpers with real platform work: awaitTerminal, sessionMetrics, triggeredWork.',
      },
      {
        k: 'first use',
        v: 'scheduled real-model runs',
        detail: 'headless, against a dedicated stack with a scoped provider key. baseline/candidate comparison is deferred until repeated runs establish variance.',
      },
      {
        k: 'release policy',
        v: 'no composite score',
        detail: 'raw metrics only; thresholds arrive only after repeated runs establish variance. a failed required check disqualifies.',
      },
    ],
  },
]

export const ADJACENT_SYSTEMS = [
  {
    name: 'HarnessBench',
    type: 'stays separate',
    desc: 'a same-prompt performance comparison product (pr #280). it intentionally omits correctness grading, multi-turn scenarios, and release gates; agent quality owns those.',
  },
  {
    name: 'workflow worker',
    type: 'stays separate',
    desc: 'a production dag orchestrator. it may be the subject of a test, but the agent-quality suite does not extend its dag or retry model.',
  },
  {
    name: 'harness::react',
    type: 'not an evaluator',
    desc: 'a lightweight event-to-agent bridge without the evidence contracts or assertions required here.',
  },
] as const
