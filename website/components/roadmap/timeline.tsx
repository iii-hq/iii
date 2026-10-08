'use client'

import { useState } from 'react'
import { DemoPlayback } from '@/components/graphics/demo-playback'
import { IconCheckCircle } from '@/components/icons/iconly'
import { usePlayhead } from '@/components/landing/use-cases-motion'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { cn } from '@/lib/utils'
import styles from './roadmap.module.css'

export type TimelineSpec = { slug: string; title: string; date: string; day: string; status: string }

/**
 * The roadmap as a build log: oldest at the top, a rail that draws down through every spec. Each live spec lights
 * and checks off as the rail reaches it; the specs still in draft sit on a dashed stretch below, their rings
 * breathing, because that is what is being written now. One playhead; the rail is a stroke-dashoffset transition and
 * the head is a transform, so it all stays on the compositor. Reduced motion shows the finished log.
 */

const ROW = 64
const STEP = 520
const START = 400
const HOLD = 3200
const OUTRO = 420

export function Timeline({ specs, className }: { specs: TimelineSpec[]; className?: string }) {
  const { ref, running, paused, setPaused, reduce } = useDemoPlayback<HTMLDivElement>()
  const [lap, setLap] = useState(0)
  /* Oldest first: the log reads down to what is being written now. */
  const rows = [...specs].reverse()
  const liveCount = rows.filter((spec) => spec.status === 'live').length
  const total = START + rows.length * STEP + HOLD + OUTRO
  const playhead = usePlayhead(running, String(lap), total, () => setLap((n) => n + 1))
  const ms = reduce ? Number.POSITIVE_INFINITY : playhead

  const reached = Math.min(rows.length, Math.max(0, Math.floor((ms - START) / STEP) + 1))
  const height = rows.length * ROW
  /* The rail runs from the first node's centre to the last's. */
  const railTop = ROW / 2
  const railLength = height - ROW
  const liveEnd = railTop + Math.max(0, liveCount - 1) * ROW
  const headY = railTop + Math.max(0, reached - 1) * ROW
  const leaving = !reduce && playhead >= total - OUTRO

  return (
    <div ref={ref} className={cn(styles.timeline, className)}>
      <div className={styles.timelineBody} data-leaving={leaving}>
        <svg
          className={styles.rail}
          viewBox={`0 0 24 ${height}`}
          preserveAspectRatio="none"
          style={{ height }}
          aria-hidden="true"
        >
          {/* Shipped: solid, drawn as far as the head has travelled. */}
          <line x1={12} x2={12} y1={railTop} y2={liveEnd} className={styles.railBase} />
          <line
            x1={12}
            x2={12}
            y1={railTop}
            y2={liveEnd}
            pathLength={1}
            className={styles.railLive}
            style={{ strokeDashoffset: 1 - Math.min(1, (headY - railTop) / Math.max(1, liveEnd - railTop)) }}
          />
          {/* In draft: a dashed stretch below the last thing that shipped. */}
          {liveCount < rows.length ? (
            <line
              x1={12}
              x2={12}
              y1={liveEnd}
              y2={railTop + railLength}
              className={styles.railDraft}
              data-on={reached > liveCount}
            />
          ) : null}
        </svg>
        <span
          aria-hidden
          className={styles.head}
          data-on={reached > 0 && reached <= liveCount}
          style={{ transform: `translateY(${headY - 4}px)` }}
        />
        <ol className={styles.log}>
          {rows.map((spec, i) => {
            const on = i < reached
            const live = spec.status === 'live'
            return (
              <li key={spec.slug} className={styles.logRow} data-on={on} data-live={live} style={{ height: ROW }}>
                <span className={styles.node} aria-hidden>
                  {live ? <IconCheckCircle className={cn(styles.nodeCheck, 'size-[18px]')} /> : null}
                </span>
                <span className={styles.logText}>
                  <span className={styles.logMeta}>
                    <time dateTime={spec.date}>{spec.day}</time>
                    <span>{live ? 'Live' : 'In draft'}</span>
                  </span>
                  <span className={styles.logTitle}>{spec.title}</span>
                </span>
              </li>
            )
          })}
        </ol>
      </div>
      <div className={styles.timelineFoot}>
        <p>
          <span>{liveCount}</span> live · <span>{rows.length - liveCount}</span> in draft
        </p>
        <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
      </div>
    </div>
  )
}
