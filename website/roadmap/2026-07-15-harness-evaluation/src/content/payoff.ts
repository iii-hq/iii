/* payoff — the scorecard, the solves table, and the honest limits. */

export const PAYOFF_METRICS = [
  { before: 'skip', after: 'setup_error', label: 'missing infrastructure becomes' },
  { before: 'model key', after: '0 keys', label: 'to reproduce a contract regression' },
  { before: 'opaque score', after: 'raw metrics', label: 'what a report shows' },
  { before: 'self-report', after: '8 oracles', label: 'evidence sources per scenario' },
] as const

export const PAYOFF_SOLVES = [
  {
    problem: 'real-model flake blocked contract regression',
    answer: 'a scripted router::* boundary',
    detail: 'stream and function-call outcomes reproduce deterministically on every pull request: ci executes each scenario twice and requires byte-identical result.json, with no model key anywhere.',
  },
  {
    problem: 'workflow quality was invisible until users hit it',
    answer: 'e2e tests on the production path',
    detail: 'a pinned model, prompt, and function catalog runs the four version 1 workflows through the production router and provider, on a schedule — plain vitest files any developer can read.',
  },
  {
    problem: 'a silent skip read as green',
    answer: 'typed failure, never a skip',
    detail: 'integration tests exit 0 only when every scenario passes, 2 for contract failures and timeouts, 3 for everything the infrastructure owns; every failed e2e test writes ordered, typed failure.json records.',
  },
  {
    problem: 'the agent graded its own work',
    answer: 'assertions over versioned evidence',
    detail: 'expect() reads durable records, tree-summed metrics, and triggered-work spans through V1 harness contracts; helpers throw on incomplete evidence, so a partial sum or a self-report never grades a run.',
  },
  {
    problem: 'one flaky check could gate forever',
    answer: 'promotion is earned',
    detail: 'required-check status only after 100 consecutive clean runs across 14 days. any unexplained flake resets the count to zero, and e2e tests stay off the pull-request gate in version 1.',
  },
] as const

export const OPEN_QUESTIONS = [
  {
    q: 'remote artifacts',
    detail: 'which artifact backend and retention policy replaces local/ci storage when evaluation runs become a shared service?',
  },
  {
    q: 'held-out + generated validators',
    detail: 'what signing and sandbox guarantees would they need before gating a release? excluded from version 1; every check is explicit code in the test file.',
  },
  {
    q: 'deeper telemetry',
    detail: 'peak context and effective prompt have no version 1 contract; unsupported rather than estimated. sub-agent and triggered-work usage is already covered by the default evidence contracts.',
  },
] as const
