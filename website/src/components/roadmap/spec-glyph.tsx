import type { CSSProperties } from "react"
import { cn } from "@/lib/utils"

// A small line drawing per spec, picked from its tags: the kind of glyph a
// developer reads at a glance. 40-unit grid, hairline strokes in the text
// colour, drawn in with the card (`.motif`). Unknown tags get the module.

type Glyph = { match: string[]; paths: string[] }

const GLYPHS: Glyph[] = [
  // typed code out of a live catalog: braces and an arrow
  {
    match: ["codegen", "types", "sdk"],
    paths: [
      "M 13 9 C 8 9 8 12 8 15 C 8 19 5 20 5 20 C 5 20 8 21 8 25 C 8 28 8 31 13 31",
      "M 27 9 C 32 9 32 12 32 15 C 32 19 35 20 35 20 C 35 20 32 21 32 25 C 32 28 32 31 27 31",
      "M 16 20 H 24 M 21 17 L 24 20 L 21 23",
    ],
  },
  // access control at the edge: a gate in front of the engine
  {
    match: ["rbac", "security", "auth"],
    paths: ["M 20 6 L 32 11 V 20 C 32 28 26 32 20 35 C 14 32 8 28 8 20 V 11 Z", "M 15 20 L 19 24 L 26 16"],
  },
  // agents: a worker with a spark inside
  {
    match: ["agents", "agent", "harness", "llm"],
    paths: [
      "M 20 6 A 14 14 0 1 0 20.01 6",
      "M 20 12 V 28 M 12 20 H 28",
      "M 14.3 14.3 L 25.7 25.7 M 25.7 14.3 L 14.3 25.7",
    ],
  },
  // compose and lifecycle: a stack of processes under one supervisor
  {
    match: ["compose", "lifecycle", "deploy", "supervisor"],
    paths: ["M 8 12 H 32 V 18 H 8 Z", "M 8 22 H 32 V 28 H 8 Z", "M 8 32 H 32", "M 12 15 H 14 M 12 25 H 14"],
  },
  // console and ui: a window with a plug coming in
  {
    match: ["console", "ui", "components"],
    paths: [
      "M 6 9 H 34 V 31 H 6 Z",
      "M 6 14 H 34",
      "M 9 11.5 H 11 M 13 11.5 H 15",
      "M 14 22 H 22 M 22 19 V 25 M 22 22 H 27",
    ],
  },
  // dx: a prompt
  { match: ["dx", "cli", "tooling"], paths: ["M 6 9 H 34 V 31 H 6 Z", "M 12 17 L 16 20 L 12 23", "M 19 23 H 27"] },
]

/** A module: a box with a port on its right edge. */
const MODULE = ["M 8 10 H 28 V 30 H 8 Z", "M 28 17 H 33 M 28 23 H 33", "M 13 15 H 23"]

function pick(tags: string[]) {
  const lower = new Set(tags.map((t) => t.toLowerCase()))
  return GLYPHS.find((g) => g.match.some((m) => lower.has(m)))?.paths ?? MODULE
}

export function SpecGlyph({ tags, className }: { tags: string[]; className?: string }) {
  const paths = pick(tags)
  const delay = (i: number) => ({ "--d": `${i * 110}ms` }) as CSSProperties
  return (
    <svg viewBox="0 0 40 40" aria-hidden="true" className={cn("motif size-10 overflow-visible", className)}>
      <g fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" strokeLinejoin="round">
        {paths.map((d, i) => (
          <path key={d} d={d} pathLength={1} data-draw="" style={delay(i)} />
        ))}
      </g>
    </svg>
  )
}
