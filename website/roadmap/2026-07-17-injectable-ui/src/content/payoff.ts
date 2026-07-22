/* payoff — the closing scorecard + problem→answer table (A11). */

export const PAYOFF_METRICS = [
  { label: 'console files per new page', before: '26', after: '0' },
  { label: 'ship a renderer', before: 'console release', after: 'worker deploy' },
  { label: 'per-worker config forms', before: 'impossible', after: 'a slot' },
  { label: 'ui change reaches tabs', before: 'rebuild + refresh', after: 'hot swap' },
] as const

export const PAYOFF_SOLVES = [
  {
    problem: 'a per-function renderer means editing FunctionCallCard.tsx and releasing the console',
    answer: 'host.functionCalls.register() from the worker',
    detail: 'injected renderers dispatch first; null falls through to the 13 built-in families, so overriding a built-in is just matching its ids.',
  },
  {
    problem: 'the memory page touched 26 console files, 23 under web/src',
    answer: 'a page is one script asset + one trigger',
    detail: 'host.pages.register mounts #/ext/<id>; presence-gating comes free because the trigger dies with the worker.',
  },
  {
    problem: 'one structural SchemaForm for every worker; no schema means no form at all',
    answer: 'configForms.register(id) overrides the form region',
    detail: 'the null-schema branch — where SchemaForm cannot render — is exactly where a custom form is worth the most. persistence stays host-owned.',
  },
  {
    problem: 'composer pickers are boolean props wired from ChatView',
    answer: 'composer.actions — additive controls, no console pr',
    detail: 'appendText + bus calls in v1; the submit path stays host-owned, deeper hooks are named v2 work.',
  },
] as const
