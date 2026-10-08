/**
 * One iii graph for the whole landing page. Every section renders the same nodes from this model and asks
 * for a `stage`; the graph shows everything that joined up to that stage, so it grows as the page scrolls.
 *
 * Layout: two columns of workers flank the engine. Each column has a vertical bus; every worker joins the bus
 * with a short horizontal stub, and the bus feeds the engine at its mid-line. New workers stack below.
 */

export const STAGES = [
  'foundation',
  'execution',
  'compose',
  'observe',
  'discover',
  'extend',
  'react',
  'summary',
  'harness',
] as const
export type Stage = (typeof STAGES)[number]
export const stageIndex = (stage: Stage) => STAGES.indexOf(stage)
export const atLeast = (stage: Stage, floor: Stage) => stageIndex(stage) >= stageIndex(floor)

/** Scripted steps for each stage: how many beats, and how long each lasts (1.5× the original tempo). */
export const CLOCK: Record<Stage, { steps: number; ms: number }> = {
  foundation: { steps: 11, ms: 974 },
  execution: { steps: 11, ms: 1230 },
  compose: { steps: 6, ms: 1950 },
  observe: { steps: 11, ms: 974 },
  discover: { steps: 7, ms: 1800 },
  extend: { steps: 6, ms: 1574 },
  react: { steps: 6, ms: 1800 },
  summary: { steps: 7, ms: 1426 },
  harness: { steps: 11, ms: 1230 },
}

export const VIEW = { w: 1040, h: 624 } as const

export type Point = { x: number; y: number }
export type Col = 'left' | 'right'

/** The 8-unit grid everything sits on. */
export const GRID = {
  nodeW: 236,
  nodeH: 56,
  pitch: 72,
  firstRow: 144,
  left: { x: 24, bus: 308 },
  right: { x: 780, bus: 732 },
  topRow: 40,
} as const

export const ENGINE = { x: 520, y: 300, w: 320, h: 68 } as const

/** Centre of a node in a column slot. */
export function place(col: Col, slot: number, h: number = GRID.nodeH) {
  const x = (col === 'left' ? GRID.left.x : GRID.right.x) + GRID.nodeW / 2
  const top = GRID.firstRow - GRID.nodeH / 2 + slot * GRID.pitch
  return { x, y: top + h / 2, w: GRID.nodeW, h, col, slot }
}
export type Placed = ReturnType<typeof place>

export type WorkerNode = Placed & {
  id: string
  /** What the node is called before it has a job (Overview). */
  role: string
  /** Worker name once it has joined (Live demo onwards). */
  worker: string
  fn: string
  kind: string
  /** Which compose file it belongs to. */
  group: 'local' | 'gpu'
  /** Where the worker came from in the Overview story. */
  origin: 'registry' | 'written' | 'human'
}

const worker = (
  id: string,
  role: string,
  w: string,
  fn: string,
  kind: string,
  col: Col,
  slot: number,
  group: 'local' | 'gpu',
  origin: WorkerNode['origin'],
): WorkerNode => ({ id, role, worker: w, fn, kind, group, origin, ...place(col, slot) })

/** Order is the Overview's arrival order; the live demo's request bounces between them through the engine. */
export const WORKERS: WorkerNode[] = [
  worker('agent', 'Agent', 'harness', 'agent::run', 'ai', 'left', 0, 'local', 'human'),
  worker('browser', 'Browser', 'browser', 'browser::navigate', 'ts', 'left', 1, 'local', 'registry'),
  worker('extract', 'Rust', 'extract', 'extract::text', 'rs', 'left', 2, 'local', 'written'),
  worker('pg', 'Database', 'postgres', 'pg::query', 'db', 'left', 3, 'local', 'human'),
  worker('github', 'TypeScript', 'github', 'github::pr::watch', 'ts', 'right', 2, 'local', 'registry'),
  worker('llm', 'Python', 'llm', 'llm::complete', 'py', 'right', 1, 'gpu', 'registry'),
  worker('embed', 'GPU', 'embed', 'embed::vectors', 'gpu', 'right', 0, 'gpu', 'written'),
]
export const workerById = Object.fromEntries(WORKERS.map((w) => [w.id, w])) as Record<string, WorkerNode>

/** Nodes that join later. */
export const LATER = {
  request: { x: GRID.left.x + 70, y: GRID.topRow, w: 140, h: 40, col: 'left' as Col },
  registry: { x: GRID.right.x + GRID.nodeW / 2, y: GRID.topRow, w: GRID.nodeW, h: 40, col: 'right' as Col },
  slack: place('left', 4),
  digest: place('left', 5),
  review: place('right', 3),
  camera: place('right', 4, 92),
  otlp: { x: ENGINE.x, y: 584, w: GRID.nodeW, h: 40 },
} as const

export const ENVIRONMENTS = ['Local', 'Cloud', 'Browser', 'Edge'] as const

/** The live demo's request, hop by hop. `code` is the line in the Code view, `ms` the span length in the Trace view. */
export const HOPS = [
  {
    node: 'agent',
    fn: 'agent::run',
    ms: 1840,
    lang: 'TypeScript',
    where: 'laptop · local',
    code: 2,
    input: '{ prompt: "review open PRs…" }',
    output: '{ digest: "3 PRs…" }',
  },
  {
    node: 'browser',
    fn: 'browser::navigate',
    ms: 412,
    lang: 'TypeScript',
    where: 'browser worker · cloud',
    code: 5,
    input: '{ url: "github.com/iii-hq/workers/pulls" }',
    output: '{ page: "#p-91" }',
  },
  {
    node: 'browser',
    fn: 'browser::act',
    ms: 288,
    lang: 'TypeScript',
    where: 'browser worker · cloud',
    code: 6,
    input: '{ page: "#p-91", action: "open files" }',
    output: '{ html: "<div …>" }',
  },
  {
    node: 'extract',
    fn: 'extract::text',
    ms: 9,
    lang: 'Rust',
    where: 'extract worker · local',
    code: 7,
    input: '{ html: "<div …>" }',
    output: '{ text: "diff --git …" }',
  },
  {
    node: 'embed',
    fn: 'embed::vectors',
    ms: 61,
    lang: 'Rust · CUDA',
    where: 'gpu box · edge',
    code: 8,
    input: '{ text: "diff --git …" }',
    output: '{ vector: [0.12, …] }',
  },
  {
    node: 'pg',
    fn: 'pg::query',
    ms: 14,
    lang: 'SQL',
    where: 'postgres worker · cloud',
    code: 9,
    input: '{ sql: "select … <-> $1", params: [[0.12, …]] }',
    output: '{ rows: 4 }',
  },
  {
    node: 'llm',
    fn: 'llm::complete',
    ms: 920,
    lang: 'Python',
    where: 'gpu box · edge',
    code: 10,
    input: '{ prompt: "Summarise …" }',
    output: '{ text: "feat: …" }',
  },
  {
    node: 'github',
    fn: 'github::pr::watch',
    ms: 33,
    lang: 'TypeScript',
    where: 'github worker · cloud',
    code: 11,
    input: '{ number: 418, on: "merged" }',
    output: '{ subscribed: true }',
  },
] as const
export type Hop = (typeof HOPS)[number]

/* ---------- Routing: everything travels along the buses, so packets follow the drawn lines exactly. ---------- */

const busX = (col: Col) => (col === 'left' ? GRID.left.bus : GRID.right.bus)
const engineEdge = (col: Col) => (col === 'left' ? ENGINE.x - ENGINE.w / 2 : ENGINE.x + ENGINE.w / 2)
/** The node edge that faces the bus. */
export const stubStart = (n: { x: number; w: number; y: number; col: Col }): Point => ({
  x: n.col === 'left' ? n.x + n.w / 2 : n.x - n.w / 2,
  y: n.y,
})

/** Polyline from a node to the engine: stub → bus → trunk. */
export function routeToEngine(n: { x: number; w: number; y: number; col: Col }): Point[] {
  const s = stubStart(n)
  const bx = busX(n.col)
  return [s, { x: bx, y: s.y }, { x: bx, y: ENGINE.y }, { x: engineEdge(n.col), y: ENGINE.y }]
}
export const routeFromEngine = (n: { x: number; w: number; y: number; col: Col }) => [...routeToEngine(n)].reverse()

/** Stub path for a node (straight line from node edge to its bus). */
export const stubPath = (n: { x: number; w: number; y: number; col: Col }) => {
  const s = stubStart(n)
  return `M ${s.x} ${s.y} H ${busX(n.col)}`
}

/** Bus path for a column: a rounded elbow at the top (from `topY`) running down to `bottomY`, plus the trunk. */
export function busPath(col: Col, topY: number, bottomY: number) {
  const bx = busX(col)
  const dir = col === 'left' ? 1 : -1
  const r = 8
  return `M ${bx - dir * r} ${topY} Q ${bx} ${topY} ${bx} ${topY + r} V ${bottomY}`
}
export const trunkPath = (col: Col) => `M ${busX(col)} ${ENGINE.y} H ${engineEdge(col)}`

/** OTLP export: straight down from the engine. */
export const otlpRoute: Point[] = [
  { x: ENGINE.x, y: ENGINE.y + ENGINE.h / 2 },
  { x: ENGINE.x, y: LATER.otlp.y - LATER.otlp.h / 2 },
]
