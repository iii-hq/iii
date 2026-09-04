export const PAYOFF_STATS = [
  { before: 'evaluation tracks', after: '2', label: 'separate questions' },
  { before: 'public entry points', after: '1', label: 'harness::send' },
  { before: 'defined scenarios', after: '9', label: '4 integration + 5 quality' },
  { before: 'model boundaries', after: '2', label: 'scripted + production' },
  { before: 'model graders', after: '0', label: 'deterministic validators' },
  { before: 'aggregate quality score', after: '0', label: 'raw dimensions in v1' },
] as const

export const SOLVES_ROWS = [
  {
    problem: 'a live model makes contract regressions noisy',
    answer: 'integration controls inference and proves exact harness behavior.',
  },
  {
    problem: 'a scripted model says nothing about real agent effectiveness',
    answer: 'quality runs the production route against a pinned subject and real-model corpus.',
  },
  {
    problem: 'a plausible final answer can hide duplicate or missing work',
    answer: 'durable state, domain outcomes, complete trees, and traces grade the run.',
  },
  {
    problem: 'one score hides whether correctness or efficiency changed',
    answer: 'quality emits a verdict and raw benchmark dimensions separately.',
  },
  {
    problem: 'infrastructure failures become silent skips',
    answer: 'both tracks fail closed with typed setup, evidence, timeout, and cleanup classes.',
  },
] as const
