export type NutshellPoint = { title: string; body: string }

export type NutshellGroup = {
  title: string
  /** one short line under the group title */
  tagline: string
  points: NutshellPoint[]
}

export const NUTSHELL_GROUPS: NutshellGroup[] = [
  {
    title: "Execution model",
    tagline: "How iii runs work: durable, interoperable, simple.",
    points: [
      {
        title: "Durable orchestration",
        body: "Coordinate long-running, failure-tolerant execution across workers and triggers.",
      },
      { title: "Interoperable execution", body: "Execute across languages natively, as if it were one runtime." },
      {
        title: "Simple primitives",
        body: "Collapse distributed backend design into a paradigm humans and agents can reason about.",
      },
    ],
  },
  {
    title: "Live system traits",
    tagline: "What iii becomes once running: discoverable, extensible, observable.",
    points: [
      {
        title: "Live discovery",
        body: "Functions and triggers exposed by one worker become visible across the system in real time.",
      },
      {
        title: "Live extensibility",
        body: "Add new workers and capabilities to a live iii system without redesigning the architecture.",
      },
      {
        title: "Live observability",
        body: "Observe operations, traces, and behavior across the entire connected stack in real time.",
      },
    ],
  },
]

export type ShapeKind = "circle" | "square" | "diamond"

export type AnimShape = {
  kind: ShapeKind
  /** resting offset from the hub, px */
  base: [number, number]
  /** offset when docked against the hub, px */
  near: [number, number]
}

/** The three workers orbiting the hub in both illustrations. */
export const ANIM_SHAPES: AnimShape[] = [
  { kind: "circle", base: [-32, 0], near: [-18, 0] },
  { kind: "square", base: [0, -32], near: [0, -18] },
  { kind: "diamond", base: [32, 0], near: [18, 0] },
]
