/* verdicts — how a test earns its verdict (A8 funnel) + status semantics. */
import type { FunnelPath } from '@lib/components/diagrams/Funnel'

export const VERDICT_PATHS: FunnelPath[] = [
  { id: 'outcomes', label: 'durable outcomes', desc: 'expect() over records' },
  { id: 'metrics', label: 'session-tree metrics', desc: 'complete, or a typed throw' },
  { id: 'spans', label: 'triggered-work spans', desc: 'present, or fail closed' },
  { id: 'budgets', label: 'budgets', desc: 'asserted token + cost caps' },
]

export const VERDICT_TARGET = {
  label: 'expect() over complete evidence',
  sub: 'a partial sum is never graded',
}

export const VERDICT_REJECT = {
  label: 'composite scores',
  desc: 'no llm judge, no blend',
}

export const STATUS_MEANINGS = [
  {
    id: 'pass',
    tone: 'accent' as const,
    desc: 'every assertion passed over complete evidence. the only green.',
  },
  {
    id: 'fail',
    tone: 'alert' as const,
    desc: 'an expect() missed: the subject failed an objective or exceeded a declared budget.',
  },
  {
    id: 'incomplete evidence',
    tone: 'warn' as const,
    desc: 'sessionMetrics or triggeredWork threw a typed error: an incomplete tree, an unreachable transcript, missing spans. never blamed on the subject as a mere fail, never graded partially.',
  },
  {
    id: 'infrastructure',
    tone: 'warn' as const,
    desc: 'a fixture, provider, or stack failure fails the run and retains evidence. never a skip.',
  },
] as const

export const NEVER_A_PASS = [
  'missing trace spans',
  'an incomplete session tree',
  'provider outage',
  'browser dropped > 0',
  'malformed evidence',
] as const
