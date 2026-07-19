/* hero — the win, quantified. data only. */

export const HERO_STATS = [
  { value: '2', label: 'tracks, one boundary' },
  { value: '1', label: 'entry point: harness::send' },
  { value: '15', label: 'frozen stream frames' },
  { value: '0', label: 'skips that read as pass' },
] as const

export const HERO_CLAIMS = [
  {
    title: 'deterministic integration',
    body: 'a scripted worker owns the router::* boundary, so stream and function-call outcomes reproduce without a model key.',
  },
  {
    title: 'real-model quality',
    body: 'a pinned model, prompt, and function catalog runs representative workflows through the production router and provider.',
  },
  {
    title: 'evidence over claims',
    body: 'code assertions over durable public evidence: send response, status, full transcript, recorder log, lifecycle events.',
  },
  {
    title: 'no silent green',
    body: 'missing infrastructure, malformed evidence, or a validator failure never becomes a passing skip. ever.',
  },
] as const
