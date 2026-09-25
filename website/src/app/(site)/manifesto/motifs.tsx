import type { CSSProperties } from "react"
import { MARK_BOX, MARK_RECTS } from "@/components/landing/hero/viz-data"
import { cn } from "@/lib/utils"

// Line drawings for the twelve statements, on a 96-unit grid: hairline
// strokes in the text colour, round caps, and every stroke normalised to
// pathLength 1 so the stylesheet can draw it in (`.motif` in globals.css).
// Filled dots arrive after the lines. Order follows manifesto-data.

type Stroke = { d: string } | { circle: [number, number, number] } | { rect: [number, number, number, number] }

/** Five points on a ring of radius r about (48, 48), starting at 12 o'clock. */
const ring = (n: number, r: number) =>
  Array.from({ length: n }, (_, i) => {
    const a = (i / n) * Math.PI * 2 - Math.PI / 2
    return [48 + r * Math.cos(a), 48 + r * Math.sin(a)] as const
  })

const mesh5 = ring(5, 32)
const meshLines: Stroke[] = mesh5.flatMap((a, i) =>
  mesh5
    .slice(i + 1)
    .map((b) => ({ d: `M ${a[0].toFixed(1)} ${a[1].toFixed(1)} L ${b[0].toFixed(1)} ${b[1].toFixed(1)}` })),
)

const MOTIFS: { strokes: Stroke[]; dots?: [number, number][] }[] = [
  // 01 systems engineering is integrations: every service wired to every other
  { strokes: meshLines, dots: mesh5.map(([x, y]) => [x, y]) },
  // 02 three primitives: a circle, a triangle, a square
  {
    strokes: [{ circle: [20, 48, 12] }, { d: "M 48 36 L 60 60 L 36 60 Z" }, { rect: [64, 36, 24, 24] }],
  },
  // 03 paradigms collapse categories: six boxes become one
  {
    strokes: [
      { rect: [10, 30, 10, 10] },
      { rect: [24, 30, 10, 10] },
      { rect: [38, 30, 10, 10] },
      { rect: [10, 56, 10, 10] },
      { rect: [24, 56, 10, 10] },
      { rect: [38, 56, 10, 10] },
      { d: "M 54 48 H 66 M 62 44 L 66 48 L 62 52" },
      { rect: [72, 36, 24, 24] },
    ],
  },
  // 04 have a need? add a worker: a ring of workers and one more joining
  {
    strokes: [{ d: "M 48 20 L 76 48 L 48 76 L 20 48 Z" }, { d: "M 76 48 L 84 22" }, { d: "M 84 12 V 32 M 74 22 H 94" }],
    dots: [
      [48, 20],
      [76, 48],
      [48, 76],
      [20, 48],
    ],
  },
  // 05 quadratic to linear: the curve that compounds, the line that doesn't
  {
    strokes: [{ d: "M 16 80 V 16 M 16 80 H 80" }, { d: "M 16 80 Q 60 80 80 22" }, { d: "M 16 74 H 80" }],
  },
  // 06 same contract, both sides: two halves, one equals
  {
    strokes: [{ d: "M 36 24 H 26 V 72 H 36" }, { d: "M 60 24 H 70 V 72 H 60" }, { d: "M 42 42 H 54 M 42 54 H 54" }],
  },
  // 07 live by default: a pulse
  {
    strokes: [{ circle: [48, 48, 12] }, { circle: [48, 48, 22] }, { circle: [48, 48, 32] }],
    dots: [[48, 48]],
  },
  // 08 any language, any runtime, one system: three shapes into one box
  {
    strokes: [
      { circle: [18, 26, 6] },
      { d: "M 18 42 L 25 54 L 11 54 Z" },
      { rect: [12, 64, 12, 12] },
      { d: "M 26 26 H 52 M 26 50 H 52 M 26 70 H 52" },
      { rect: [56, 22, 28, 52] },
    ],
  },
  // 09 humans and agents share one mental model: two circles, one centre
  {
    strokes: [{ circle: [36, 48, 20] }, { circle: [60, 48, 20] }],
    dots: [[48, 48]],
  },
  // 10 agents are workers: the same circle, with a spark inside
  {
    strokes: [{ circle: [48, 48, 24] }, { d: "M 48 34 V 62 M 34 48 H 62" }, { d: "M 38 38 L 58 58 M 58 38 L 38 58" }],
  },
  // 11 compose::add is the npm moment: a prompt
  {
    strokes: [{ rect: [12, 24, 72, 48] }, { d: "M 24 40 L 32 48 L 24 56" }, { d: "M 40 56 H 56" }],
  },
  // 12 add a worker: the mark, three times
  {
    strokes: [
      { rect: [26, 30, 8, 8] },
      { rect: [26, 46, 8, 20] },
      { rect: [44, 30, 8, 8] },
      { rect: [44, 46, 8, 20] },
      { rect: [62, 30, 8, 8] },
      { rect: [62, 46, 8, 20] },
    ],
  },
]

/** The motif for statement `index`, 96×96, drawn in once its cell enters view. */
export function Motif({ index, className }: { index: number; className?: string }) {
  const m = MOTIFS[index]
  if (!m) return null
  const delay = (i: number) => ({ "--d": `${i * 90}ms` }) as CSSProperties
  return (
    <svg viewBox="0 0 96 96" aria-hidden="true" className={cn("motif size-24 overflow-visible", className)}>
      <g fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" strokeLinejoin="round">
        {m.strokes.map((s, i) => {
          if ("circle" in s) {
            const [cx, cy, r] = s.circle
            return (
              <circle
                key={`c${s.circle.join(",")}`}
                cx={cx}
                cy={cy}
                r={r}
                pathLength={1}
                data-draw=""
                style={delay(i)}
              />
            )
          }
          if ("rect" in s) {
            const [x, y, w, h] = s.rect
            return (
              <rect
                key={`r${s.rect.join(",")}`}
                x={x}
                y={y}
                width={w}
                height={h}
                rx={2}
                pathLength={1}
                data-draw=""
                style={delay(i)}
              />
            )
          }
          return <path key={s.d} d={s.d} pathLength={1} data-draw="" style={delay(i)} />
        })}
      </g>
      {m.dots?.map(([x, y], i) => (
        <circle
          key={`d${x},${y}`}
          cx={x}
          cy={y}
          r={3}
          fill="currentColor"
          data-fill=""
          style={delay(m.strokes.length + i)}
        />
      ))}
    </svg>
  )
}

/**
 * The hero diagram: the three primitives as three overlapping circles with a
 * single point where all three meet. Hairlines draw in on load; the labels
 * and the centre arrive after. Reads as one figure, in the manner of a
 * construction drawing.
 */
/** The mark's side in drawing units, at the centre of the primitives figure. */
const MARK_UNITS = 38

export function Primitives({ className }: { className?: string }) {
  const delay = (i: number) => ({ "--d": `${i * 160}ms` }) as CSSProperties
  // The box hugs the three circles and their labels, so the figure fills its
  // column instead of floating in dead space.
  const mark = MARK_UNITS / MARK_BOX
  return (
    <svg viewBox="110 0 500 320" aria-hidden="true" className={cn("motif overflow-visible", className)}>
      <g fill="none" stroke="currentColor" strokeWidth="1.5">
        <circle
          cx="250"
          cy="160"
          r="128"
          pathLength={1}
          data-draw=""
          style={delay(0)}
          transform="rotate(-90 250 160)"
        />
        <circle
          cx="360"
          cy="160"
          r="128"
          pathLength={1}
          data-draw=""
          style={delay(1)}
          transform="rotate(-90 360 160)"
        />
        <circle
          cx="470"
          cy="160"
          r="128"
          pathLength={1}
          data-draw=""
          style={delay(2)}
          transform="rotate(-90 470 160)"
        />
        {/* construction line through the centres, broken around the mark */}
        <path
          d={`M 250 160 H ${360 - MARK_UNITS / 2 - 12} M ${360 + MARK_UNITS / 2 + 12} 160 H 470`}
          pathLength={1}
          data-draw=""
          style={delay(3)}
          className="[stroke-dasharray:1] opacity-50"
        />
      </g>
      {/* Labels sit above each circle's apex; too small to read once the figure is phone-width, so hidden there. */}
      <g className="fill-gray-10 font-mono text-[13px] tracking-[0.04em] max-sm:hidden">
        <text x="250" y="16" textAnchor="middle" data-fill="" style={delay(4)}>
          worker
        </text>
        <text x="360" y="308" textAnchor="middle" data-fill="" style={delay(4)}>
          trigger
        </text>
        <text x="470" y="16" textAnchor="middle" data-fill="" style={delay(4)}>
          function
        </text>
      </g>
      {/* The mark where all three overlap: the engine the primitives meet in. */}
      <g data-fill="" style={delay(5)} transform={`translate(${360 - MARK_UNITS / 2} ${160 - MARK_UNITS / 2})`}>
        <g transform={`scale(${mark})`} className="fill-gray-12">
          {MARK_RECTS.map((r) => (
            <rect key={`${r.x}-${r.y}`} x={r.x} y={r.y} width={r.width} height={r.height} />
          ))}
        </g>
      </g>
    </svg>
  )
}
