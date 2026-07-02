/* payoff — the closing scorecard + problem→answer table (A11). */

export const PAYOFF_METRICS = [
  { label: 'discovery fns filtered', before: '2/8', after: '8/8' },
  { label: 'auth-bug blast radius', before: 'engine', after: 'proxy' },
  { label: 'engines it can front', before: '1', after: 'any' },
  { label: 'drifting rule copies', before: '2', after: '0' },
] as const

export const PAYOFF_SOLVES = [
  {
    problem: 'gated workers still enumerate what they cannot call',
    answer: 'all eight discovery results rewritten to the caller',
    detail: 'the same is_function_allowed gates discovery and invocation, so the two can never disagree.',
  },
  {
    problem: 'an auth bug takes the engine down with it',
    answer: 'auth runs in a separate process',
    detail: 'the blast radius of a policy bug is the proxy, not the engine behind it.',
  },
  {
    problem: 'you cannot guard an engine you do not operate',
    answer: 'just a worker pointed at an engine url',
    detail: 'front a remote or managed engine with your own access policy.',
  },
  {
    problem: 'two copies of the rules drift apart',
    answer: 'decision logic vendored from the engine',
    detail: 'a fixture test asserts the proxy matcher equals the engine matcher, byte-for-byte.',
  },
] as const
