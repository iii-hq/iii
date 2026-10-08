import { story } from './content'

export const STORY_STAGES = ['compose', 'observe', 'discover', 'extend', 'react'] as const
export type StoryStage = (typeof STORY_STAGES)[number]
export type StoryNodeId =
  | 'http'
  | 'api'
  | 'database'
  | 'directory'
  | 'storage'
  | 'router'
  | 'anthropic'
  | 'openai'
  | 'harness'
  | 'github'

/** Selected workers, not a complete manifest. All execution edges go through iii.
 * Contracts: docs/next/using-iii/compose.mdx; iii-hq/workers manifests and
 * iii-directory/src/functions/search.rs; github/README.md (checked 2026-10-06).
 * api and the three event handlers are illustrative application code.
 */
export const STORY_NODES: {
  id: StoryNodeId
  name: string
  detail: string
  x: number
  y: number
  stage: StoryStage
  at: number
}[] = [
  { id: 'http', name: 'http', detail: 'HTTP endpoints', x: 18, y: 18, stage: 'compose', at: 1 },
  { id: 'api', name: 'api', detail: 'Python · your code', x: 50, y: 7, stage: 'compose', at: 3 },
  { id: 'database', name: 'database', detail: 'SQL queries', x: 82, y: 18, stage: 'compose', at: 2 },
  { id: 'directory', name: 'iii-directory', detail: 'Find capabilities', x: 82, y: 44, stage: 'discover', at: 1 },
  { id: 'storage', name: 'storage', detail: 'Store objects', x: 82, y: 73, stage: 'extend', at: 1 },
  { id: 'router', name: 'llm-router', detail: 'Route model calls', x: 18, y: 73, stage: 'extend', at: 2 },
  { id: 'anthropic', name: 'provider-anthropic', detail: 'Model provider', x: 18, y: 44, stage: 'extend', at: 3 },
  { id: 'openai', name: 'provider-openai', detail: 'Model provider', x: 50, y: 69, stage: 'extend', at: 3 },
  { id: 'harness', name: 'harness', detail: 'Agent runtime', x: 50, y: 92, stage: 'extend', at: 4 },
  { id: 'github', name: 'github', detail: 'PR events', x: 50, y: 26, stage: 'react', at: 1 },
]

export const ENGINE_POINT = { x: 50, y: 46 }
export const STORY_BEAT_MS = 1150
/** How long a chapter's finished state holds before it loops back to the start. */
export const STORY_REST_MS = 3200
/** Observability reads at a glance (charts, then three spans), so its beats run quicker than the graph chapters. */
const OBSERVE_BEAT_MS = 720
/** Beats that play their own small timeline (typing, a search walking a list, a tree resolving) get room to finish. */
const BEAT_TIMING: Partial<Record<StoryStage, readonly number[]>> = {
  discover: [1900, 1300, 1800, 1950, 1400, 2100, 1500, 1700],
  extend: [1150, 850, 1100, 650, 600, 800, 750],
  react: [1300, 1200, 1000, 1000, 1600, 1500],
}
export function storyBeatDuration(stage: StoryStage, beat: number) {
  if (stage === 'observe') return OBSERVE_BEAT_MS
  return BEAT_TIMING[stage]?.[beat] ?? STORY_BEAT_MS
}
export const STORY_STEPS = STORY_STAGES.map((id) => ({ id, ...story[id] }))

type Beat = { caption: string; active: StoryNodeId[]; route?: StoryNodeId[] }
export const STORY_BEATS: Record<StoryStage, Beat[]> = {
  compose: [
    { caption: 'Start with the engine. Add only what you need.', active: [] },
    { caption: 'Compose starts the HTTP worker.', active: ['http'] },
    { caption: 'The database joins the same engine.', active: ['database'] },
    { caption: 'Your Python API joins alongside the registry workers.', active: ['api'] },
    { caption: 'An HTTP request reaches your function through iii.', active: ['http', 'api'], route: ['http', 'api'] },
    {
      caption: 'Your Python function calls the database through iii.',
      active: ['api', 'database'],
      route: ['api', 'database'],
    },
    { caption: 'Three workers. Different languages. One connected system.', active: ['http', 'api', 'database'] },
  ],
  observe: [
    { caption: 'Inspect function calls across your iii workers.', active: [] },
    { caption: 'See completed and failed function calls over time.', active: [] },
    { caption: 'An HTTP request starts a trace.', active: [] },
    { caption: 'Trace context travels into your Python function.', active: [] },
    { caption: 'The database call belongs to the same trace.', active: [] },
    { caption: 'One trace, across languages and machines. Export it over OTLP.', active: [] },
  ],
  discover: [
    { caption: 'You ask the harness for something in plain words.', active: [] },
    { caption: 'directory::search_functions splits the prompt into three capabilities.', active: ['directory'] },
    { caption: 'It searches what is already running. database::execute matches.', active: ['directory'] },
    { caption: 'Nothing running archives files.', active: ['directory', 'database'] },
    { caption: 'The worker registry has storage. Install it and call it.', active: ['directory', 'database'] },
    { caption: 'Nothing running applies a loyalty discount.', active: ['directory', 'database'] },
    { caption: 'No registry worker does it either.', active: ['directory', 'database'] },
    { caption: 'So you write it: your own worker, loyalty::apply.', active: ['directory', 'database'] },
    {
      caption: 'Call one, install one, write one. All three end up in the same system.',
      active: ['directory', 'database'],
    },
  ],
  extend: [
    { caption: 'Ask for what the system is missing.', active: [] },
    { caption: 'Compose knows what each worker depends on.', active: [] },
    { caption: 'One compose::add call asks for both workers.', active: [] },
    { caption: 'storage has no dependencies. It joins on its own.', active: [] },
    { caption: 'harness joins, and Compose resolves what it needs.', active: [] },
    { caption: 'Its dependencies join the graph with it.', active: [] },
    { caption: 'llm-router brings the model providers it routes to.', active: [] },
    { caption: 'Twelve new workers, one request. Any worker can call them through iii.', active: [] },
  ],
  react: [
    { caption: 'Three handlers subscribe to github::pr::event.', active: [] },
    { caption: 'A pull request opens on GitHub.', active: [] },
    { caption: 'The github worker emits github::pr::event through iii.', active: [] },
    { caption: 'iii delivers the event to every subscriber at once.', active: [] },
    { caption: 'Each handler reacts on its own: a review, a message, a test run.', active: [] },
    { caption: 'Add a fourth handler. The event source does not change.', active: [] },
    { caption: 'One event, four reactions. Nothing upstream was edited.', active: [] },
  ],
}

export function lastStoryBeat(stage: StoryStage) {
  return STORY_BEATS[stage].length - 1
}

export function storyNodeVisible(node: (typeof STORY_NODES)[number], stage: StoryStage, beat: number) {
  const stageIndex = STORY_STAGES.indexOf(stage)
  const joinedIndex = STORY_STAGES.indexOf(node.stage)
  return joinedIndex < stageIndex || (joinedIndex === stageIndex && beat >= node.at)
}

/** The example request's spans; `at` is the trace beat (0–2) on which the span is the one running. */
export const STORY_TRACE = [
  { id: 'http', name: 'POST /orders', meta: 'Rust · cloud', depth: 0, start: 0, duration: 128, at: 0 },
  { id: 'api', name: 'orders::create', meta: 'Python · local', depth: 1, start: 16, duration: 92, at: 1 },
  { id: 'database', name: 'database::execute', meta: 'Rust · cloud', depth: 2, start: 42, duration: 48, at: 2 },
] as const
/** Trace clock (ms) per Observability beat: idle while the charts draw, then the request runs to 128 ms. */
export const OBSERVE_ELAPSED = [0, 0, 34, 80, 112, 128] as const
