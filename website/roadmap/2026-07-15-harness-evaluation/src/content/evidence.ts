import type { FunnelPath } from '@lib/components/diagrams/Funnel'

export type EvidenceTrack = 'integration' | 'quality'

export const INTEGRATION_EVIDENCE_PATHS: FunnelPath[] = [
  { id: 'send-status', label: 'send + status', desc: 'accepted · completed · no pending work' },
  { id: 'transcript', label: 'full transcript', desc: 'ordered · paginated · no duplicate ids' },
  { id: 'router', label: 'router history', desc: '12 request fields · every generation consumed' },
  { id: 'traces', label: 'session traces', desc: 'target + lifecycle · no pending or error spans' },
  { id: 'process', label: 'process state', desc: 'early exits · bounded teardown · groups reaped' },
]

export const QUALITY_EVIDENCE_PATHS: FunnelPath[] = [
  { id: 'outcome', label: 'domain outcome', desc: 'fixture records · exactly-once effects · deterministic checks' },
  { id: 'durable', label: 'status + transcript', desc: 'requested turn terminal · complete active path' },
  { id: 'tree', label: 'session tree', desc: 'root + descendants · complete parentage' },
  { id: 'metrics', label: 'metrics', desc: 'time · tokens · cost · turns · calls · errors' },
  { id: 'triggered', label: 'triggered work', desc: 'hooks · triggers · sub-agents · downstream spans' },
]

export const COMMON_EVIDENCE_RULES = [
  {
    name: 'completion signal',
    type: 'hint',
    desc: 'lifecycle delivery wakes collection but durable status confirms the requested turn.',
  },
  {
    name: 'agent answer',
    type: 'not an oracle',
    desc: 'the subject cannot make its own run pass by claiming success.',
  },
  {
    name: 'trace completeness',
    type: 'required',
    desc: 'open, malformed, missing, or dropped subject spans fail closed.',
  },
  {
    name: 'private state + logs',
    type: 'diagnostic only',
    desc: 'they can explain a failure but cannot convert it into a pass.',
  },
] as const

export const INTEGRATION_FLOOR_ROWS = [
  { name: 'terminal', type: 'status', desc: 'completed with pending calls, queue, and children empty.' },
  { name: 'router', type: 'exact consumption', desc: 'every scripted generation matches and no extra call appears.' },
  { name: 'traces', type: 'turn identity', desc: 'complete clean trees equal the expected terminal-turn count.' },
  { name: 'teardown', type: 'bounded', desc: 'process groups terminate and reap inside the hard budget.' },
] as const

export const QUALITY_FLOOR_ROWS = [
  {
    name: 'correctness',
    type: 'domain + harness',
    desc: 'scenario outcomes and durable harness state satisfy structured checks.',
  },
  {
    name: 'attribution',
    type: 'complete tree',
    desc: 'root and descendant usage and work map to the right subject trace.',
  },
  { name: 'budgets', type: 'hard ceilings', desc: 'time, tokens, cost, calls, and errors may fail the scenario.' },
  {
    name: 'benchmark',
    type: 'raw observation',
    desc: 'variation below ceilings remains visible and is not collapsed into a score.',
  },
] as const
