/* payoff — the scorecard, the solves table, and the honest limits. */

export const PAYOFF_METRICS = [
  { before: 'skip', after: 'setup_error', label: 'missing infrastructure becomes' },
  { before: 'model key', after: '0 keys', label: 'to reproduce a contract regression' },
  { before: 'opaque score', after: 'raw deltas', label: 'what a comparison reports' },
  { before: 'self-report', after: '8 oracles', label: 'evidence sources per scenario' },
] as const

export const PAYOFF_SOLVES = [
  {
    problem: 'real-model flake blocked contract regression',
    answer: 'a scripted router::* boundary',
    detail: 'stream and function-call outcomes reproduce deterministically on every pull request: the same two scenarios, byte-normalized, with or without a key.',
  },
  {
    problem: 'workflow quality was invisible until users hit it',
    answer: 'the harness-eval worker',
    detail: 'a pinned model, prompt, and function catalog runs representative workflows through the production path, on a schedule.',
  },
  {
    problem: 'a silent skip read as green',
    answer: 'a no-skip policy with exit codes',
    detail: 'exit 0 only when every scenario passes; 2 for contract failures and timeouts; 3 for everything the infrastructure owns.',
  },
  {
    problem: 'the agent graded its own work',
    answer: 'independent, versioned validators',
    detail: 'held-out validators the subject can never see; artifact tokens scoped per attempt and revoked after each call; harness-eval::* denied to the subject.',
  },
  {
    problem: 'one flaky check could gate forever',
    answer: 'promotion is earned',
    detail: 'required-check status only after 100 consecutive clean runs across 14 days. any unexplained flake resets the count to zero.',
  },
] as const

export const OPEN_QUESTIONS = [
  {
    q: 'remote artifacts',
    detail: 'which artifact backend and retention policy replaces local/ci storage when evaluation becomes a shared service?',
  },
  {
    q: 'agent-authored validators',
    detail: 'what signing and sandbox guarantees are required before one may participate in a release gate? in v1 they run in disposable secret-free stacks and never gate alone.',
  },
  {
    q: 'deeper telemetry',
    detail: 'peak context and effective prompt are not durably persisted today; unsupported in v1 rather than estimated. sub-agent and triggered-work usage is required in every report.',
  },
] as const
