/* verdicts — the one aggregation rule (A8 funnel) + status semantics. */
import type { FunnelPath } from '@lib/components/diagrams/Funnel'

export const VERDICT_PATHS: FunnelPath[] = [
  { id: 'validators', label: 'required validators', desc: 'every one, in every cycle' },
  { id: 'cycles', label: 'cycles', desc: 'bounded feedback rounds' },
  { id: 'attempts', label: 'attempts', desc: '1–10 per leg' },
  { id: 'legs', label: 'comparison legs', desc: 'baseline + candidate' },
]

export const VERDICT_TARGET = {
  label: 'error > fail > inconclusive > pass',
  sub: 'the same fold at every level',
}

export const VERDICT_REJECT = {
  label: 'weighted composite score',
  desc: 'no blended number gates a release in v1',
}

export const STATUS_MEANINGS = [
  {
    id: 'pass',
    tone: 'accent' as const,
    desc: 'every required validator in every attempt passed. the only green.',
  },
  {
    id: 'fail',
    tone: 'alert' as const,
    desc: 'the subject missed an objective, including exhausting a declared budget.',
  },
  {
    id: 'error',
    tone: 'warn' as const,
    desc: 'the evaluator, a fixture, the provider, or a validator dependency broke. never blamed on the subject.',
  },
  {
    id: 'inconclusive',
    tone: 'warn' as const,
    desc: 'evidence cannot support a decision: missing traces, dropped browser entries, a validator timeout.',
  },
] as const

export const NEVER_A_PASS = [
  'missing traces',
  'provider outage',
  'browser dropped > 0',
  'malformed validator results',
  'validator timeouts',
] as const
