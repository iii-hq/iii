// Static geometry and per-stage settings for the hero visualization
// ("problem space" → "current" → "iii"). Everything here is plain data so the
// SVG renders on the server pass too; only the packets are animated on the client.

export type Stage = "mesh" | "actual" | "iii"

export type VizNode = {
  x: number
  y: number
  /** position on the ring, radians, 0 = 3 o'clock */
  angle: number
  label: string
  /** width of the service chip drawn at (x, y), from the label's length */
  w: number
}

export type Edge = readonly [number, number]

/** SVG viewBox size (square). */
export const VIEWBOX = 400
const CX = VIEWBOX / 2
const CY = VIEWBOX / 2
/** ring radius */
const R = 152

/** Service chips: a rounded rect with a status dot and the label inside. */
export const CHIP_H = 24
export const CHIP_R = 7
/** Inter at 11px, weight 500: about this wide per average character; m/w run wide, i/l/t/r/f/j narrow. */
const CHAR_W = 6.1
const CHIP_PAD = 22
const CHIP_DOT = 10
const textWidth = (label: string) =>
  [...label].reduce((w, ch) => w + ("mw".includes(ch) ? 1.5 : "iltrfj".includes(ch) ? 0.6 : 1), 0) * CHAR_W

const NODE_LABELS = ["agent", "orchestrator", "db", "cache", "queue", "stream", "http", "cron", "obs", "memory"]

// Round so the server and client render identical attribute strings (no hydration drift).
const round = (n: number) => Math.round(n * 1000) / 1000

/** Nodes around the ring, starting at 12 o'clock and going clockwise. */
export const NODES: VizNode[] = NODE_LABELS.map((label, i) => {
  const angle = (i / NODE_LABELS.length) * Math.PI * 2 - Math.PI / 2
  const cos = Math.cos(angle)
  return {
    x: round(CX + R * cos),
    y: round(CY + R * Math.sin(angle)),
    angle,
    label,
    w: Math.round(textWidth(label) + CHIP_DOT + CHIP_PAD),
  }
})

/** Every pairwise edge: n(n-1)/2 = 45. */
export const MESH_EDGES: Edge[] = NODES.flatMap((_, i) => NODES.slice(i + 1).map((_, k) => [i, i + 1 + k] as const))

/** The hub: where the engine sits in the iii stage. */
export const CENTER = { x: CX, y: CY }

/** How far a chord's control point is pulled toward the centre (0 = straight). */
const BOW = 0.45

/**
 * A chord between two ring nodes as a quadratic curve bowed toward the centre.
 * Opposite nodes bow to a straight line through the middle; neighbours barely
 * bend. Curves read as a woven ring instead of a wire cage.
 */
export function chordPath(i: number, j: number) {
  const a = NODES[i]
  const b = NODES[j]
  const mx = (a.x + b.x) / 2
  const my = (a.y + b.y) / 2
  const cx = round(mx + (CX - mx) * BOW)
  const cy = round(my + (CY - my) * BOW)
  return `M ${a.x} ${a.y} Q ${cx} ${cy} ${b.x} ${b.y}`
}

/** Radius of the engine disc at the hub. Spokes stop at its edge, never inside it. */
export const HUB_R = 30

/** The point on the hub's edge that faces node `i`. */
export function hubEdge(i: number) {
  const n = NODES[i]
  return { x: round(CX + HUB_R * Math.cos(n.angle)), y: round(CY + HUB_R * Math.sin(n.angle)) }
}

/** A service's permanent spoke: from the hub's edge out to the node. */
export function spokePath(i: number) {
  const e = hubEdge(i)
  const n = NODES[i]
  return `M ${e.x} ${e.y} L ${n.x} ${n.y}`
}

/**
 * A call through the engine, as three pieces of path data: the leg into the
 * hub, the leg out of it, and both legs as one path (with a jump between
 * them) for the light to ride. The share of the whole trip that the first leg
 * takes is `split`, so timings can line up with the hub.
 */
export function callPaths(i: number, j: number) {
  const a = NODES[i]
  const b = NODES[j]
  const ea = hubEdge(i)
  const eb = hubEdge(j)
  const legIn = `M ${a.x} ${a.y} L ${ea.x} ${ea.y}`
  const legOut = `M ${eb.x} ${eb.y} L ${b.x} ${b.y}`
  return { legIn, legOut, ride: `${legIn} ${legOut}`, split: 0.5 }
}

/** The curated "what teams actually wire up" subgraph: 20 edges. */
export const ACTUAL_EDGES: Edge[] = [
  [0, 1],
  [0, 3],
  [0, 5],
  [0, 6],
  [0, 7],
  [3, 4],
  [1, 8],
  [5, 9],
  [2, 6],
  [7, 8],
  [1, 2],
  [2, 3],
  [4, 5],
  [4, 6],
  [6, 9],
  [3, 8],
  [0, 2],
  [0, 9],
  [5, 7],
  [1, 4],
]

export const STAGE_ORDER: Stage[] = ["mesh", "actual", "iii"]

type StageConfig = {
  /** stage title in the window's narrative column */
  name: string
  /** the hero copy that tells this stage's part of the story */
  body: readonly { text: string; em?: boolean }[]
  /** tab label */
  tab: string
  /** how long the auto-cycle holds this stage (ms); the resolution gets longest */
  dwell: number
  /** integrations count in the footer */
  count: number
  /** paint the count in the brand color */
  accentCount: boolean
  /** packet spawn interval (ms) */
  spawnEvery: number
  /** max packets in flight */
  maxPackets: number
}

export const STAGES: Record<Stage, StageConfig> = {
  mesh: {
    name: "Problem space",
    body: [
      { text: "Modern software stacks are an exercise in integrating services. " },
      { text: "Every new capability means a new system to learn, configure, deploy, and monitor." },
    ],
    tab: "Problem space",
    dwell: 6000,
    count: MESH_EDGES.length,
    accentCount: false,
    spawnEvery: 150,
    maxPackets: 16,
  },
  actual: {
    name: "Implementation",
    body: [
      { text: "The complexity of actual integrations is " },
      { text: "quadratic", em: true },
      {
        text: " which is overwhelming for devs and for AI. Leading to both making bad decisions about large codebases.",
      },
    ],
    tab: "Current",
    dwell: 6000,
    count: ACTUAL_EDGES.length,
    accentCount: false,
    spawnEvery: 280,
    maxPackets: 8,
  },
  iii: {
    name: "Solved.",
    body: [
      { text: "iii " },
      { text: "eliminates this complexity", em: true },
      { text: ". Every service plugs into one engine, so using a new one is as easy as importing a library." },
    ],
    tab: "With iii",
    dwell: 9000,
    count: 0,
    accentCount: true,
    spawnEvery: 720,
    maxPackets: 3,
  },
}

/** Packet travel time: base + random jitter (ms). Eased, so it needs a little longer than a linear trip. */
export const PACKET_BASE_MS = 1100
export const PACKET_JITTER_MS = 500
export const PACKET_RADIUS = 2.5

/** The iii mark's own geometry (1075.74 box), drawn at MARK_SIZE user units inside the hub. */
export const MARK_SIZE = 20
export const MARK_BOX = 1075.74
export const MARK_RECTS = [
  { x: 0, y: 0.05, width: 268.94, height: 268.94 },
  { x: 0, y: 403.45, width: 268.94, height: 672.24 },
  { x: 403.4, y: 0.05, width: 268.94, height: 268.94 },
  { x: 403.4, y: 403.45, width: 268.94, height: 672.24 },
  { x: 806.81, y: 0.05, width: 268.94, height: 268.94 },
  { x: 806.81, y: 403.45, width: 268.94, height: 672.24 },
] as const

// ---- Complexity chart ----
// A 4:1 canvas drawn at its own aspect (no stretching), so strokes and the end
// dot stay true. Curves are single quadratic Béziers: a parabola y = a·x² from
// the origin is exactly a Q with its control point on the baseline under the
// midpoint, so the draw-in follows the real curve, not a polyline.

export const CHART_W = 400
export const CHART_H = 100
/** the x axis, in chart units from the top */
export const CHART_BASE = 90
export const CHART_VIEWBOX = `0 0 ${CHART_W} ${CHART_H}`

export type Curve = {
  id: "quadratic" | "damped" | "flat"
  /** the stroke */
  d: string
  /** the same curve closed to the baseline, for the fill under the active one */
  area: string
  /** where the curve ends, for the dot */
  end: { x: number; y: number }
  /** which stage draws this curve as the active one */
  stage: Stage
}

const parabola = (endY: number): Pick<Curve, "d" | "area" | "end"> => ({
  d: `M 0 ${CHART_BASE} Q ${CHART_W / 2} ${CHART_BASE} ${CHART_W} ${endY}`,
  area: `M 0 ${CHART_BASE} Q ${CHART_W / 2} ${CHART_BASE} ${CHART_W} ${endY} L ${CHART_W} ${CHART_BASE} Z`,
  end: { x: CHART_W, y: endY },
})

export const CURVES: Curve[] = [
  // n(n-1)/2: full pairwise growth
  { id: "quadratic", stage: "mesh", ...parabola(14) },
  // the same growth, damped by how many integrations a team tolerates
  { id: "damped", stage: "actual", ...parabola(56) },
  // zero: flat on the axis
  {
    id: "flat",
    stage: "iii",
    d: `M 0 ${CHART_BASE} L ${CHART_W} ${CHART_BASE}`,
    area: `M 0 ${CHART_BASE} L ${CHART_W} ${CHART_BASE} Z`,
    end: { x: CHART_W, y: CHART_BASE },
  },
]
