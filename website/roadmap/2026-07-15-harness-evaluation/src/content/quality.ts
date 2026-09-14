export const QUALITY_SUBJECT_FIELDS = [
  { name: 'model + provider', type: 'resolved', desc: 'the production route and exact model are fixed for the run.' },
  {
    name: 'system prompt',
    type: 'bytes + strategy',
    desc: 'enrich or override plus the SHA-256 of the resolved UTF-8 prompt.',
  },
  { name: 'skills', type: 'exposed set', desc: 'the run records which capabilities are visible to the subject.' },
  { name: 'artifacts', type: 'digests', desc: 'harness and worker identities make the subject reproducible.' },
  {
    name: 'options',
    type: 'bounded',
    desc: 'thinking, output limits, provider options, and scenario budgets stay explicit.',
  },
] as const

export const QUALITY_EVIDENCE = [
  {
    name: 'domain outcome',
    type: 'scenario-specific',
    desc: 'fixture records prove the effect independently from the agent claim.',
  },
  {
    name: 'status + transcript',
    type: 'durable',
    desc: 'the requested turn reaches terminal state with a complete active message path.',
  },
  { name: 'session tree', type: 'complete', desc: 'root and descendant sessions preserve parentage and attribution.' },
  {
    name: 'metrics',
    type: 'aggregated',
    desc: 'time, tokens, cost, turns, calls, and errors cover the full session tree.',
  },
  {
    name: 'triggered work',
    type: 'trace-backed',
    desc: 'hooks, triggers, sub-agents, and downstream calls fail closed on incomplete spans.',
  },
] as const

export const QUALITY_SCENARIOS = [
  {
    id: '01',
    slug: 'plain response',
    outcome: 'durable final text with no duplicate assistant entry',
    proof: ['terminal transcript', 'zero duplicate entries', 'reported usage'],
  },
  {
    id: '02',
    slug: 'single function',
    outcome: 'one allowed domain effect reaches the next generation',
    proof: ['effect exactly once', 'result reaches model', 'call budget'],
  },
  {
    id: '03',
    slug: 'security review',
    outcome: 'sub-agents inspect disjoint partitions and the parent deduplicates findings',
    proof: ['fan-out / fan-in', 'durable findings', 'usage by session'],
  },
  {
    id: '04',
    slug: 'triggered work',
    outcome: 'reactive orchestration completes with error-free descendant spans',
    proof: ['declared trigger', 'complete parentage', 'zero dropped spans'],
  },
  {
    id: '05',
    slug: 'functional reduction',
    outcome: 'a large result is reduced before the next model generation',
    proof: ['source-to-pipeline trace', 'reduced payload only', 'token ceiling'],
  },
] as const

export const QUALITY_OUTPUTS = [
  {
    name: 'correctness verdict',
    type: 'deterministic',
    desc: 'structured checks over durable domain and harness evidence.',
  },
  {
    name: 'benchmark observation',
    type: 'raw dimensions',
    desc: 'wall time, tokens, cost, turns, calls, errors, tree size, and triggered work.',
  },
  {
    name: 'aggregate score',
    type: 'absent in v1',
    desc: 'correctness and efficiency are not collapsed into one quality number.',
  },
] as const
