'use client'

import { useState } from 'react'

import { usePlayhead } from '@/components/a/use-cases-motion'
import { DemoPlayback } from '@/components/graphics/demo-playback'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { cn } from '@/lib/utils'
import { hero } from './data'
import styles from './manifesto.module.css'

/**
 * The manifesto's argument in one loop, told with the hero's own three lines:
 *   "software complexity continues to grow." six services wired every-to-every, fifteen integrations drawn in.
 *   "remove it." the mesh unwinds; one engine arrives and each service becomes a worker with one connection.
 *   "add a worker." a seventh joins with a single line, and everything else stays as it was.
 * One playhead drives it; the drawing is CSS transitions on stroke-dashoffset (pathLength 1), so it stays on the
 * compositor. Reduced motion shows the finished picture.
 */

const VIEW = { w: 520, h: 440 }
const C = { x: 260, y: 214 }
const R = 168
const CHIP = { w: 108, h: 30 }
/* Real registry workers. The seventh slot stays empty until the end. */
const NAMES = ['http', 'queue', 'cron', 'state', 'database', 'harness', 'sandbox'] as const
const SLOTS = NAMES.map((_, i) => {
  const a = (i / NAMES.length) * Math.PI * 2 - Math.PI / 2
  return { x: C.x + R * Math.cos(a), y: C.y + R * 0.86 * Math.sin(a) }
})
const SIX = SLOTS.slice(0, 6)
const MESH = SIX.flatMap((a, i) => SIX.slice(i + 1).map((b) => ({ a, b })))

/* Timeline (ms) */
const NODE_IN = (i: number) => 80 + i * 70
const MESH_AT = (i: number) => 600 + i * 75
const MESH_DONE = MESH_AT(MESH.length - 1) + 420
const UNWIND = MESH_DONE + 900
const ENGINE_IN = UNWIND + 260
const SPOKE_AT = (i: number) => ENGINE_IN + 280 + i * 80
const JOIN = SPOKE_AT(5) + 1400
const JOIN_SPOKE = JOIN + 420
const HOLD_END = JOIN_SPOKE + 3000
const TOTAL = HOLD_END + 420

export function Collapse({ className }: { className?: string }) {
  const { ref, running, paused, setPaused, reduce } = useDemoPlayback<HTMLDivElement>()
  const [lap, setLap] = useState(0)
  const playhead = usePlayhead(running, String(lap), TOTAL, () => setLap((n) => n + 1))
  const ms = reduce ? Number.POSITIVE_INFINITY : playhead

  const unwound = ms >= UNWIND
  const meshDrawn = MESH.filter((_, i) => ms >= MESH_AT(i)).length
  const phase = ms >= JOIN ? 2 : unwound ? 1 : 0
  const spokes = SIX.filter((_, i) => ms >= SPOKE_AT(i)).length + (ms >= JOIN_SPOKE ? 1 : 0)
  const count =
    phase === 0
      ? { workers: 6, links: meshDrawn, noun: 'integrations' }
      : { workers: phase === 2 ? 7 : 6, links: spokes, noun: 'connections' }
  const caption = [hero.problem, hero.verb, hero.answer][phase]
  const leaving = !reduce && playhead >= HOLD_END

  return (
    <div ref={ref} className={cn(styles.collapse, className)}>
      <svg
        viewBox={`0 0 ${VIEW.w} ${VIEW.h}`}
        role="img"
        aria-label="Six services wired to each other with fifteen integrations collapse into one iii engine with one connection each; a seventh worker then joins with a single connection."
        className={styles.collapseSvg}
        data-leaving={leaving}
      >
        {/* Every service wired to every other: the integration tax. */}
        <g className={styles.mesh} data-unwound={unwound}>
          {MESH.map(({ a, b }, i) => (
            <line
              key={`${a.x}-${b.x}-${a.y}-${b.y}`}
              x1={a.x}
              y1={a.y}
              x2={b.x}
              y2={b.y}
              pathLength={1}
              data-on={ms >= MESH_AT(i)}
              style={{ transitionDelay: unwound ? `${(MESH.length - i) * 18}ms` : undefined }}
            />
          ))}
        </g>

        {/* One engine, one connection per worker. */}
        <g className={styles.spokes}>
          {SLOTS.map((slot, i) => (
            <line
              key={NAMES[i]}
              x1={C.x}
              y1={C.y}
              x2={slot.x}
              y2={slot.y}
              pathLength={1}
              data-on={i < 6 ? ms >= SPOKE_AT(i) : ms >= JOIN_SPOKE}
              data-new={i === 6}
            />
          ))}
        </g>

        <g className={styles.engine} data-on={ms >= ENGINE_IN}>
          <rect x={C.x - 54} y={C.y - 26} width={108} height={52} rx={14} />
          {/* The iii mark: three dots over three bars. */}
          {[0, 1, 2].map((k) => (
            <g key={k} className={styles.mark}>
              <rect x={C.x - 33 + k * 9} y={C.y - 11} width={5} height={5} />
              <rect x={C.x - 33 + k * 9} y={C.y - 3} width={5} height={14} />
            </g>
          ))}
          <text x={C.x + 2} y={C.y + 4} className={styles.engineLabel}>
            engine
          </text>
        </g>

        {SLOTS.map((slot, i) => (
          <g
            key={NAMES[i]}
            className={styles.node}
            data-on={i < 6 ? ms >= NODE_IN(i) : ms >= JOIN}
            data-new={i === 6}
            style={{ transformOrigin: `${slot.x}px ${slot.y}px` }}
          >
            <rect x={slot.x - CHIP.w / 2} y={slot.y - CHIP.h / 2} width={CHIP.w} height={CHIP.h} rx={8} />
            <text x={slot.x} y={slot.y + 4} className={styles.nodeLabel}>
              {NAMES[i]}
            </text>
            {i === 6 ? <circle cx={slot.x + CHIP.w / 2 - 12} cy={slot.y} r={3} className={styles.ready} /> : null}
          </g>
        ))}
      </svg>

      <div className={styles.collapseFoot}>
        <p className={styles.collapseCaption} aria-live="polite">
          <span key={caption} className="swap-in">
            {caption}
          </span>
        </p>
        <p className={styles.collapseCount}>
          <span>{count.workers}</span> {phase === 0 ? 'services' : 'workers'} ·{' '}
          <span key={`${phase}-${count.links}`} className="swap-in">
            {count.links}
          </span>{' '}
          {count.noun}
        </p>
        <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
      </div>
    </div>
  )
}
