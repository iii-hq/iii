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
        v: 'proposed',
        detail: 'delivery starts only after the public durable harness::session-tree dependency exists; the integration runner does not host or emulate this evaluator.',
      },
      {
        k: 'model boundary',
        v: 'production router + pinned model',
        detail: 'the same harness, router, provider, and function boundaries used in production, with pinned inputs.',
      },
      {
        k: 'primary oracle',
        v: 'versioned validators',
        detail: 'deterministic outcome checks plus raw quality, reliability, latency, token, and cost dimensions.',
      },
      {
        k: 'execution owner',
        v: 'harness-eval worker',
        detail: 'one dedicated worker with a durable run record: orchestration, validation, evidence, reports.',
      },
      {
        k: 'first use',
        v: 'scheduled + comparison runs',
        detail: 'single-scenario runs and baseline/candidate comparisons with a persisted interleaved schedule.',
      },
      {
        k: 'release policy',
        v: 'no composite score',
        detail: 'raw deltas only; thresholds arrive only after repeated runs establish variance. safety failures disqualify.',
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
    desc: 'a production dag orchestrator. it may be the subject of a scenario, but harness-eval does not extend its dag or retry model.',
  },
  {
    name: 'harness::react',
    type: 'not an evaluator',
    desc: 'a lightweight event-to-agent bridge without an experiment record, validation protocol, retries, or report aggregation.',
  },
] as const
