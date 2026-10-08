import type { TraceSpan } from './trace-waterfall'

/**
 * The harness session the Proof section replays. One prompt that needs every kind of capability the section's copy
 * names: a browser on your machine, a remote sandbox, a model through llm-router, session state, and GitHub. Function
 * ids follow the landing doc and the registry workers (browser, iii-sandbox, llm-router, state, github). Sample data.
 */

export const PROMPT = 'Checkout is failing on staging. Find out why and open a fix.'
export const REPLY =
  'Checkout failed because the payment retry was never awaited. I reproduced it in a sandbox, patched it, and opened PR #512. The tests pass.'

export const SEARCH = { fn: 'directory::search_functions', ms: 3 } as const

export const CALLS = [
  {
    fn: 'browser::navigate',
    where: 'browser · your laptop',
    result: 'staging /checkout returns 500',
    ms: 840,
    start: 20,
  },
  {
    fn: 'sandbox::exec',
    where: 'iii-sandbox · remote machine',
    result: '1 failing test: retry not awaited',
    ms: 1920,
    start: 880,
  },
  {
    fn: 'router::chat',
    where: 'llm-router · provider-anthropic',
    result: 'Patch drafted',
    ms: 1340,
    start: 2820,
  },
  { fn: 'state::set', where: 'state · this session', result: 'Root cause saved', ms: 6, start: 4170 },
  { fn: 'github::pr::create', where: 'github · iii-hq/shop', result: 'PR #512 opened', ms: 410, start: 4180 },
] as const

export const TURN_MS = 4600

/** One span in the inspector. `worker` picks the bar colour; `label` says where the name sits relative to its bar. */
export type ProofSpan = TraceSpan & { worker: string; parent?: string; label: 'inside' | 'right' | 'left' }

/** The trace the right-hand inspector draws: the turn, each call as a child, and the provider stream under the router. */
export const SPANS: ProofSpan[] = [
  { id: 'turn', name: 'harness::turn', worker: 'harness', depth: 0, start: 0, duration: TURN_MS, label: 'inside' },
  {
    id: 'browser::navigate',
    name: 'browser::navigate',
    worker: 'browser',
    parent: 'turn',
    depth: 1,
    start: 20,
    duration: 840,
    label: 'right',
  },
  {
    id: 'sandbox::exec',
    name: 'sandbox::exec',
    worker: 'iii-sandbox',
    parent: 'turn',
    depth: 1,
    start: 880,
    duration: 1920,
    label: 'inside',
  },
  {
    id: 'router::chat',
    name: 'router::chat',
    worker: 'llm-router',
    parent: 'turn',
    depth: 1,
    start: 2820,
    duration: 1340,
    label: 'inside',
  },
  {
    id: 'provider::anthropic::stream',
    name: 'provider::anthropic::stream',
    worker: 'provider-anthropic',
    parent: 'router::chat',
    depth: 2,
    start: 2860,
    duration: 1240,
    label: 'left',
  },
  {
    id: 'state::set',
    name: 'state::set',
    worker: 'state',
    parent: 'turn',
    depth: 1,
    start: 4170,
    duration: 6,
    label: 'left',
  },
  {
    id: 'github::pr::create',
    name: 'github::pr::create',
    worker: 'github',
    parent: 'turn',
    depth: 1,
    start: 4180,
    duration: 410,
    label: 'left',
  },
]

/** A quiet hue per worker (low chroma, so the bars read as categories, not alarms). Green is kept for "done". */
export const WORKER_COLORS: Record<string, string> = {
  harness: 'oklch(0.66 0.11 278)',
  browser: 'oklch(0.72 0.08 232)',
  'iii-sandbox': 'oklch(0.76 0.09 80)',
  'llm-router': 'oklch(0.72 0.11 48)',
  'provider-anthropic': 'oklch(0.7 0.09 25)',
  state: 'oklch(0.7 0.02 260)',
  github: 'oklch(0.7 0.08 310)',
}

/** Nearest-rank percentile over the spans that have finished. */
export function percentile(values: number[], p: number) {
  if (!values.length) return 0
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.max(0, Math.ceil((p / 100) * sorted.length) - 1)]
}

/* Beats: 0 prompt types, 1 sent, 2 search, 3–7 one per call, 8 reply streams, 9 finished (held). */
export const SEARCH_AT = 2
export const CALL_AT = 3
export const REPLY_AT = CALL_AT + CALLS.length
export const DONE_AT = REPLY_AT + 1
export const BEAT_MS = [1500, 700, 1300, 1000, 1150, 1050, 800, 900, 1700] as const
export const REST_MS = 3800
/** Within a search or call beat, the share of the beat spent running before the result lands. */
export const RUN_SHARE = 0.55

/** Trace clock per beat: nothing until the first call, then each call's end. */
export const ELAPSED = [0, 0, 0, ...CALLS.map((call) => call.start + call.ms), TURN_MS, TURN_MS] as const

/** Context window in thousands of tokens: the system prompt, then the five matched schemas, then each result. */
export const CONTEXT_K = [1.2, 1.2, 1.9, 2.2, 2.5, 2.9, 2.9, 3.1, 3.4, 3.4] as const
