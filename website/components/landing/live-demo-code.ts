import type { BundledLanguage } from 'shiki'

/**
 * Illustrative playback, not a connection to a running engine. Registry contracts checked against
 * github/src/functions/pr.rs and llm-router/src/{chat/chat,types/router}.rs in iii-hq/workers.
 * The orchestrator is application code; the other three nodes are registry workers.
 */
export const demoCode = {
  file: 'orchestrator.ts',
  lang: 'ts' as BundledLanguage,
  lines: [
    "import { registerWorker } from 'iii-sdk'",
    "const iii = registerWorker('ws://localhost:49134')",
    '',
    "iii.registerFunction('digest::run', async ({ repo }: { repo: string }) => {",
    '  const prs = await iii.trigger({',
    "    function_id: 'github::pr::list',",
    "    payload: { repo, state: 'open', limit: 3 },",
    '  })',
    '',
    '  const result = await iii.trigger({',
    "    function_id: 'router::complete',",
    '    payload: {',
    "      model: 'claude-sonnet-4',",
    "      system_prompt: 'Summarize these open PRs.',",
    "      messages: [{ role: 'user',",
    "        content: [{ type: 'text', text: JSON.stringify(prs.value) }],",
    '        timestamp: Date.now() }],',
    '    },',
    '    timeoutMs: 320_000,',
    '  })',
    '',
    '  return result.message',
    '})',
  ],
}

export type DemoWorkerId = 'orchestrator' | 'github' | 'router' | 'provider'
export type Endpoint = DemoWorkerId | 'request'

export const DEMO_WORKERS = [
  {
    id: 'orchestrator',
    name: 'orchestrator',
    fn: 'digest::run',
    language: 'TypeScript',
    location: 'Local',
    x: 25,
    y: 20,
    code: [3, 3],
    start: 0,
    duration: 1560,
    depth: 0,
  },
  {
    id: 'github',
    name: 'github',
    fn: 'github::pr::list',
    language: 'Rust',
    location: 'Cloud',
    x: 75,
    y: 20,
    code: [4, 7],
    start: 80,
    duration: 420,
    depth: 1,
  },
  {
    id: 'router',
    name: 'llm-router',
    fn: 'router::complete',
    language: 'Rust',
    location: 'Cloud',
    x: 25,
    y: 80,
    code: [9, 19],
    start: 500,
    duration: 1000,
    depth: 1,
  },
  {
    id: 'provider',
    name: 'provider-anthropic',
    fn: 'provider::anthropic::stream',
    language: 'Rust',
    location: 'Cloud',
    x: 75,
    y: 80,
    code: [9, 19],
    start: 580,
    duration: 860,
    depth: 2,
  },
] as const
export type DemoWorker = (typeof DEMO_WORKERS)[number]
export const demoWorkerById = Object.fromEntries(DEMO_WORKERS.map((worker) => [worker.id, worker])) as Record<
  DemoWorkerId,
  DemoWorker
>

type Beat = {
  from: Endpoint
  to: Endpoint
  worker: DemoWorkerId
  title: string
  detail: string
  elapsed: number
  reply?: boolean
  code?: readonly [number, number]
}
export const DEMO_BEATS: readonly Beat[] = [
  {
    from: 'request',
    to: 'orchestrator',
    worker: 'orchestrator',
    title: 'A request arrives.',
    detail: 'iii finds the worker that registered digest::run.',
    elapsed: 80,
  },
  {
    from: 'orchestrator',
    to: 'github',
    worker: 'github',
    title: 'Fetch the open PRs.',
    detail: 'Your TypeScript function calls the GitHub worker in Rust.',
    elapsed: 250,
  },
  {
    from: 'github',
    to: 'orchestrator',
    worker: 'github',
    title: 'Three PRs come back.',
    detail: 'The reply returns through iii to the waiting function.',
    elapsed: 500,
    reply: true,
  },
  {
    from: 'orchestrator',
    to: 'router',
    worker: 'router',
    title: 'Ask for a summary.',
    detail: 'The same call interface reaches the LLM router.',
    elapsed: 580,
  },
  {
    from: 'router',
    to: 'provider',
    worker: 'provider',
    title: 'The router calls its provider.',
    detail: 'Worker-to-worker calls also pass through iii.',
    elapsed: 1050,
  },
  {
    from: 'provider',
    to: 'router',
    worker: 'provider',
    title: 'The response streams back.',
    detail: 'The provider returns the summary to the router.',
    elapsed: 1440,
    reply: true,
  },
  {
    from: 'router',
    to: 'orchestrator',
    worker: 'router',
    title: 'The summary is ready.',
    detail: 'The router returns the complete assistant message.',
    elapsed: 1500,
    reply: true,
  },
  {
    from: 'orchestrator',
    to: 'request',
    worker: 'orchestrator',
    title: 'Return the digest.',
    detail: 'One request, four workers, one connected execution.',
    elapsed: 1560,
    reply: true,
    code: [21, 21],
  },
]
export const DEMO_DONE = DEMO_BEATS.length
export const DEMO_DURATION = 1560
export const DEMO_BEAT_MS = 1600
/** How long a packet takes to cross the graph. The rest of the beat holds on the receiver, lit, so it can be read. */
export const DEMO_TRAVEL_MS = 1100
export const DEMO_RESULT_HOLD_MS = 4000

export const DEMO_RESULT = [
  { number: 431, title: 'Retry webhook delivery', description: 'Retries failed deliveries with backoff.' },
  { number: 432, title: 'Add query filters', description: 'Adds filtering to database queries.' },
  { number: 433, title: 'Improve worker startup', description: 'Makes startup failures easier to diagnose.' },
] as const
