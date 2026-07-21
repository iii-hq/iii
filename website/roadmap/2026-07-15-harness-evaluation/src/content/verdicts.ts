/* verdicts — how an e2e test earns its verdict (A8 funnel) + the six typed
   failure classes of AgentQualityFailureReportV1. */
import type { FunnelPath } from '@lib/components/diagrams/Funnel'

export const VERDICT_PATHS: FunnelPath[] = [
  { id: 'outcomes', label: 'durable outcomes', desc: 'expect() over records' },
  { id: 'metrics', label: 'session-tree metrics', desc: 'complete, or a typed throw' },
  { id: 'spans', label: 'triggered-work spans', desc: 'closed, zero dropped' },
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

export const FAILURE_CLASSES = [
  {
    id: 'setup_error',
    phase: 'setup',
    tone: 'warn' as const,
    desc: 'a required worker, function, trigger type, provider route, model version, fixture, or config entry is unavailable before the first subject send. function_not_found or FORBIDDEN on a setup probe lands here — never retried as transient.',
  },
  {
    id: 'subject_error',
    phase: 'send',
    tone: 'alert' as const,
    desc: 'harness::send or the terminal turn reports a non-timeout execution failure or cancellation.',
  },
  {
    id: 'assertion_failure',
    phase: 'assert',
    tone: 'alert' as const,
    desc: 'a domain-state, transcript, metric, or span assertion evaluates false after complete evidence collection.',
  },
  {
    id: 'evidence_error',
    phase: 'collect',
    tone: 'warn' as const,
    desc: 'an evidence response is missing, malformed, incomplete, open, truncated, or reports dropped entries. never blamed on the subject, never graded partially.',
  },
  {
    id: 'timeout',
    phase: 'send · await',
    tone: 'alert' as const,
    desc: 'the subject exceeds the turn or test deadline, including engine timeout or sdk TIMEOUT during send/await.',
  },
  {
    id: 'cleanup_error',
    phase: 'cleanup',
    tone: 'warn' as const,
    desc: 'fixture teardown, harness::stop, terminal confirmation, or sdk shutdown fails or exceeds its deadline. appended after the original failure, never replacing it.',
  },
] as const

export const NEVER_A_PASS = [
  'missing or dropped trace spans',
  'an incomplete session tree',
  'provider outage',
  'browser dropped > 0',
  'a sampled subject trace',
  'malformed evidence',
] as const
