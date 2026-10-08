'use client'

import { motion } from 'motion/react'
import { useId } from 'react'

type Point = readonly [number, number]
type Face = { points: Point[]; glyphs: string }

const project = (x: number, y: number, z: number): Point => [58 + x * 1.08 + z * 0.8, 67 + y + x * 0.21 - z * 0.6]

function inside(x: number, y: number, polygon: Point[]) {
  let hit = false
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const a = polygon[i]
    const b = polygon[j]
    if (a[1] > y !== b[1] > y && x < ((b[0] - a[0]) * (y - a[1])) / (b[1] - a[1]) + a[0]) hit = !hit
  }
  return hit
}

// Rasterize the six extruded parts of the iii mark into real ASCII characters.
const faces: Face[] = []
for (const x of [0, 54, 108]) {
  for (const [y, height] of [
    [0, 34],
    [52, 102],
  ]) {
    const a = project(x, y, 0)
    const b = project(x + 34, y, 0)
    const c = project(x + 34, y + height, 0)
    const d = project(x, y + height, 0)
    const e = project(x, y, 27)
    const f = project(x + 34, y, 27)
    const g = project(x + 34, y + height, 27)
    faces.push(
      { points: [a, b, c, d], glyphs: '##*+' },
      { points: [b, f, g, c], glyphs: '::/:' },
      { points: [a, e, f, b], glyphs: '+-=+' },
    )
  }
}

const rows = Array.from({ length: 43 }, (_, row) => {
  let text = ''
  for (let col = 0; col < 64; col++) {
    const x = col * 4.5
    const y = row * 5.7 + 25
    const face = faces.findLast((candidate) => inside(x, y, candidate.points))
    text += face ? face.glyphs[(row * 7 + col * 3) % face.glyphs.length] : ' '
  }
  return { id: `ascii-${row}`, y: row * 5.7 + 25, text }
})

/** The iii engine, built from ASCII, scans a request through its three extruded stems. */
export function HeroAsciiCore({ running, sequence }: { running: boolean; sequence: number }) {
  const id = useId()
  return (
    <svg viewBox="0 0 290 320" className="h-auto w-full" aria-hidden="true">
      <defs>
        <clipPath id={id}>
          <motion.rect
            key={sequence}
            x="0"
            width="290"
            height="65"
            initial={false}
            animate={running ? { y: [0, 270] } : { y: 110 }}
            transition={running ? { duration: 2.9, ease: 'linear', repeat: Number.POSITIVE_INFINITY } : { duration: 0 }}
          />
        </clipPath>
      </defs>
      <path d="M24 239 144 191 271 241 151 291Z" fill="var(--background)" stroke="var(--line-strong)" />
      <path d="M24 239v11l127 52 120-50v-11M151 291v11" fill="none" stroke="var(--line)" />
      <path
        d="m42 232 109 44 101-42M58 256v6m15 0v6m15 0v6m15 0v6m15 0v6m65-8v7m16-14v7m16-14v7m16-14v7"
        fill="none"
        stroke="var(--line-strong)"
      />
      {faces.map((face) => {
        const points = face.points.map((point) => point.join(',')).join(' ')
        return <polygon key={points} points={points} fill="var(--background)" />
      })}
      <text xmlSpace="preserve" className="fill-muted-foreground font-mono" fontSize="7.4" letterSpacing="0.04">
        {rows.map((row) => (
          <tspan key={row.id} x="0" y={row.y}>
            {row.text}
          </tspan>
        ))}
      </text>
      <g clipPath={`url(#${id})`}>
        <text xmlSpace="preserve" className="fill-hero-accent font-mono" fontSize="7.4" letterSpacing="0.04">
          {rows.map((row) => (
            <tspan key={row.id} x="0" y={row.y}>
              {row.text}
            </tspan>
          ))}
        </text>
      </g>
    </svg>
  )
}
