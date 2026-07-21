/* hero — the win, quantified. data only. */

export const HERO_STATS = [
  { value: '2', label: 'tracks, one public boundary' },
  { value: '6', label: 'tests gate version 1' },
  { value: '15', label: 'frozen stream frames' },
  { value: '0', label: 'skips that read as pass' },
] as const

export const HERO_CLAIMS = [
  {
    title: 'deterministic integration',
    body: 'a scripted worker owns the router::* boundary, so stream and function-call outcomes reproduce without a model key.',
  },
  {
    title: 'real-model e2e',
    body: 'plain vitest files drive a pinned model, prompt, and function catalog through the production router and provider.',
  },
  {
    title: 'evidence over claims',
    body: 'code assertions over durable public evidence: transcript, status, session tree, tree-summed metrics, triggered-work spans.',
  },
  {
    title: 'no silent green',
    body: 'missing infrastructure, malformed evidence, or a failed check never becomes a passing skip. ever.',
  },
] as const
