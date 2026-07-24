export const EVALUATION_TRACKS = [
  {
    id: 'integration',
    label: 'integration',
    question: 'did the harness contract hold?',
    boundary: 'scripted router, no provider network',
    oracle: 'exact assertions over public durable evidence',
    corpus: '4 fixtures: 2 direct + 2 Console/playground',
    cadence: 'pull requests and deterministic regression',
    output: 'result.json + execution.json + teardown.json',
  },
  {
    id: 'quality',
    label: 'quality / e2e',
    question: 'did the pinned agent achieve the outcome inside its budget?',
    boundary: 'production router + provider + pinned subject',
    oracle: 'domain assertions plus complete harness evidence',
    corpus: '5 materially different real-model scenarios',
    cadence: 'local, scheduled, on-demand, and release candidate',
    output: 'correctness verdict + benchmark observation',
  },
] as const

export const SHARED_CONTRACTS = [
  {
    name: 'entry',
    type: 'harness::send',
    desc: 'both tracks enter through the same public Function and real queue-owned turn.',
  },
  {
    name: 'authority',
    type: 'durable state',
    desc: 'status, transcript, domain records, and traces outrank lifecycle timing and self-report.',
  },
  { name: 'validation', type: 'deterministic code', desc: 'neither track asks a model to grade its own answer.' },
  {
    name: 'failure',
    type: 'fail closed',
    desc: 'missing infrastructure or incomplete evidence never becomes a passing skip.',
  },
] as const
