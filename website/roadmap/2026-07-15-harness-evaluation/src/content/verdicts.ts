import type { RevealStage } from '@lib/components/diagrams/StepReveal'

export const VERDICT_STAGES: RevealStage[] = [
  {
    label: 'pass',
    tone: 'accent',
    caption: 'the selected track completed its public evidence floor, scenario checks, budgets, and cleanup.',
    rows: [
      { k: 'integration', v: 'contract verdict passed' },
      { k: 'quality', v: 'correctness passed + benchmark present' },
    ],
  },
  {
    label: 'subject / contract',
    tone: 'alert',
    caption: 'the stack ran, but the harness contract or configured subject outcome failed its deterministic checks.',
    rows: [
      { k: 'integration', v: 'contract_failure' },
      { k: 'quality', v: 'subject_error or assertion_error' },
    ],
  },
  {
    label: 'evidence',
    tone: 'alert',
    caption: 'required durable records, metrics, or traces were incomplete, malformed, open, or dropped.',
    rows: [
      { k: 'rule', v: 'partial data never passes' },
      { k: 'quality', v: 'evidence_error' },
    ],
  },
  {
    label: 'timeout',
    tone: 'warn',
    caption: 'the requested subject phase exceeded its monotonic deadline and cleanup still had to finish.',
    rows: [
      { k: 'integration', v: 'subject timeout in Await' },
      { k: 'quality', v: 'scenario or Function budget' },
    ],
  },
  {
    label: 'setup / process',
    tone: 'alert',
    caption:
      'required binaries, workers, credentials, configuration, or process health failed before a valid verdict existed.',
    rows: [
      { k: 'classification', v: 'setup_error or process_crash' },
      { k: 'skip', v: 'never converted to pass' },
    ],
  },
  {
    label: 'cleanup / runner',
    tone: 'alert',
    caption: 'artifact persistence, evidence collection, session stop, or teardown failed and takes final precedence.',
    rows: [
      { k: 'classification', v: 'cleanup_error or runner_error' },
      { k: 'precedence', v: 'highest' },
    ],
  },
]

export const INTEGRATION_REPORTS = [
  { name: 'result.json', type: 'stable', desc: 'scenario, classification, scrubbed failure, and artifact paths.' },
  { name: 'execution.json', type: 'volatile', desc: 'run identity, timestamps, duration, result path, and SHA-256.' },
  { name: 'teardown.json', type: 'always', desc: 'typed process signal, reap, and deadline outcomes.' },
] as const

export const QUALITY_REPORTS = [
  {
    name: 'correctness verdict',
    type: 'required',
    desc: 'structured scenario assertions over complete durable evidence.',
  },
  {
    name: 'benchmark.json',
    type: 'passing run',
    desc: 'raw time, tokens, cost, turns, calls, errors, and tree dimensions.',
  },
  { name: 'results.json', type: 'compact run', desc: 'subject identity plus one result entry per corpus scenario.' },
] as const
