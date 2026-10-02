'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'

import { useGraphicLoop } from '@/components/graphics/use-graphic-loop'
import { easeOut } from '@/lib/motion'

import { EngineHub, Flow, Node, STROKE, T, Wire } from './graph/kit'
import {
  busPath,
  type Col,
  ENGINE,
  GRID,
  place,
  routeFromEngine,
  routeToEngine,
  stubPath,
  trunkPath,
  VIEW,
} from './graph/model'
import { GraphScroller } from './graph/scroller'

const BEAT_MS = 1300

/**
 * One workload's slice of the iii graph, drawn with the same kit and grid as the big graph: the engine in the
 * middle, the workload's functions in two columns wired to it by stubs and a bus. One call runs through them in
 * order, each hop out to a worker and back through the engine, then the complete graph holds for a beat.
 */
export function MiniGraph({ fns, label }: { fns: readonly string[]; label: string }) {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const [tick, setTick] = useState(0)

  useEffect(() => {
    setTick(0)
    if (!active) return
    const id = window.setInterval(() => setTick((t) => t + 1), BEAT_MS)
    return () => window.clearInterval(id)
  }, [active])

  const beats = fns.length + 1
  const step = active ? tick % beats : fns.length
  const cycle = Math.floor(tick / beats)
  const running = step < fns.length

  const nodes = fns.map((fn, i) => {
    const col: Col = i % 2 === 0 ? 'left' : 'right'
    return { fn, worker: fn.split('::')[0], ...place(col, Math.floor(i / 2)) }
  })
  const extent = (col: Col) => {
    const ys = nodes.filter((n) => n.col === col).map((n) => n.y)
    return { top: Math.min(ENGINE.y, ...ys), bottom: Math.max(ENGINE.y, ...ys) }
  }
  const left = extent('left')
  const right = extent('right')

  /* Crop the shared canvas to the rows this workload uses. */
  const top = GRID.firstRow - GRID.nodeH / 2 - 20
  const bottom = Math.max(left.bottom, right.bottom) + GRID.nodeH / 2 + 20
  const current = running ? nodes[step] : null
  const caption = current ? current.fn : 'every call passes through the engine'

  return (
    <div ref={ref} className="graphic-stage min-w-0">
      <GraphScroller>
        <svg
          viewBox={`0 ${top} ${VIEW.w} ${bottom - top}`}
          role="img"
          aria-label={label}
          className="h-auto w-full overflow-visible"
        >
          {/* Buses and trunks */}
          <path
            d={busPath('left', left.top, left.bottom)}
            fill="none"
            stroke="var(--line-strong)"
            strokeWidth={STROKE}
            strokeLinecap="round"
          />
          <path
            d={busPath('right', right.top, right.bottom)}
            fill="none"
            stroke="var(--line-strong)"
            strokeWidth={STROKE}
            strokeLinecap="round"
          />
          <Wire d={trunkPath('left')} />
          <Wire d={trunkPath('right')} />
          {nodes.map((n, i) => (
            <Wire key={n.fn} d={stubPath(n)} delay={0.05 * i} />
          ))}

          <EngineHub pulseKey={running ? `${cycle}-${step}` : undefined} lit={running} />

          {nodes.map((n, i) => {
            const lit = step === i
            const done = step > i
            return (
              <Node
                key={n.fn}
                cx={n.x}
                cy={n.y}
                w={n.w}
                h={n.h}
                title={n.worker}
                sub={n.fn}
                meta={lit ? 'running' : done ? 'ok' : undefined}
                metaTone={lit ? 'accent' : 'muted'}
                bar={lit || done ? 1 : 0}
                barLit={lit}
                tone={lit ? 'lit' : 'idle'}
                delay={0.08 * i}
              />
            )
          })}

          {/* One call: out to the worker, back through the engine */}
          <AnimatePresence>
            {active && current ? (
              <motion.g key={`${cycle}-${step}`} exit={{ opacity: 0 }} transition={{ duration: 0.2 }}>
                <Flow points={routeFromEngine(current)} duration={0.32} />
                <Flow points={routeToEngine(current)} delay={0.42} duration={0.32} />
              </motion.g>
            ) : null}
          </AnimatePresence>
        </svg>
      </GraphScroller>
      <div
        aria-hidden
        className="mt-3 flex h-5 items-center justify-center overflow-hidden text-center font-sans text-[13px] text-muted-foreground"
      >
        <AnimatePresence mode="wait" initial={false}>
          <motion.span
            key={caption}
            className={current ? 'truncate font-mono' : 'truncate'}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.3 * T, ease: easeOut }}
          >
            {caption}
          </motion.span>
        </AnimatePresence>
      </div>
    </div>
  )
}
