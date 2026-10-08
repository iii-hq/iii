'use client'

import { useInView } from 'motion/react'
import { type CSSProperties, useRef } from 'react'

import { cn } from '@/lib/utils'
import styles from './manifesto.module.css'

/*
 * Line drawings for the twelve statements, the live iii.dev's own (website/src/app/(site)/manifesto/motifs.tsx), on a
 * 96-unit grid. Every stroke is normalised to pathLength 1 and drawn in when its statement arrives on screen: lines
 * first, 70ms apart, then the filled dots. Drawn once; reduced motion shows them finished.
 */

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
    strokes: [{ circle: [20, 48, 12] }, { d: 'M 48 36 L 60 60 L 36 60 Z' }, { rect: [64, 36, 24, 24] }],
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
      { d: 'M 54 48 H 66 M 62 44 L 66 48 L 62 52' },
      { rect: [72, 36, 24, 24] },
    ],
  },
  // 04 have a need? add a worker: a ring of workers and one more joining
  {
    strokes: [{ d: 'M 48 20 L 76 48 L 48 76 L 20 48 Z' }, { d: 'M 76 48 L 84 22' }, { d: 'M 84 12 V 32 M 74 22 H 94' }],
    dots: [
      [48, 20],
      [76, 48],
      [48, 76],
      [20, 48],
    ],
  },
  // 05 quadratic to linear: the curve that compounds, the line that doesn't
  {
    strokes: [{ d: 'M 16 80 V 16 M 16 80 H 80' }, { d: 'M 16 80 Q 60 80 80 22' }, { d: 'M 16 74 H 80' }],
  },
  // 06 same contract, both sides: two halves, one equals
  {
    strokes: [{ d: 'M 36 24 H 26 V 72 H 36' }, { d: 'M 60 24 H 70 V 72 H 60' }, { d: 'M 42 42 H 54 M 42 54 H 54' }],
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
      { d: 'M 18 42 L 25 54 L 11 54 Z' },
      { rect: [12, 64, 12, 12] },
      { d: 'M 26 26 H 52 M 26 50 H 52 M 26 70 H 52' },
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
    strokes: [{ circle: [48, 48, 24] }, { d: 'M 48 34 V 62 M 34 48 H 62' }, { d: 'M 38 38 L 58 58 M 58 38 L 38 58' }],
  },
  // 11 compose::add is the npm moment: a prompt
  {
    strokes: [{ rect: [12, 24, 72, 48] }, { d: 'M 24 40 L 32 48 L 24 56' }, { d: 'M 40 56 H 56' }],
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

/** The drawing for statement `index`, drawn in once it enters the viewport. */
export function Motif({ index, className }: { index: number; className?: string }) {
  const ref = useRef<SVGSVGElement>(null)
  const inView = useInView(ref, { once: true, margin: '0px 0px -15% 0px' })
  const m = MOTIFS[index]
  if (!m) return null
  const delay = (i: number) => ({ '--d': `${i * 70}ms` }) as CSSProperties
  return (
    <svg ref={ref} viewBox="0 0 96 96" aria-hidden="true" className={cn(styles.motif, className)} data-drawn={inView}>
      <g fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        {m.strokes.map((s, i) => {
          if ('circle' in s) {
            const [cx, cy, r] = s.circle
            return <circle key={`c${s.circle.join(',')}`} cx={cx} cy={cy} r={r} pathLength={1} style={delay(i)} />
          }
          if ('rect' in s) {
            const [x, y, w, h] = s.rect
            return (
              <rect
                key={`r${s.rect.join(',')}`}
                x={x}
                y={y}
                width={w}
                height={h}
                rx={2}
                pathLength={1}
                style={delay(i)}
              />
            )
          }
          return <path key={s.d} d={s.d} pathLength={1} style={delay(i)} />
        })}
      </g>
      {m.dots?.map(([x, y], i) => (
        <circle
          key={`d${x},${y}`}
          cx={x}
          cy={y}
          r={2.5}
          className={styles.motifDot}
          style={delay(m.strokes.length + i)}
        />
      ))}
    </svg>
  )
}
