'use client'

import { type CSSProperties, type ReactNode, useEffect, useRef } from 'react'

import { IconActivity, IconCheckCircle, IconChevronRight } from '@/components/icons/iconly'
import { cn } from '@/lib/utils'
import styles from './trace-waterfall.module.css'

export type TraceSpan = {
  id: string
  /** Function id, shown in mono. */
  name: string
  /** Nesting under the root span; each level indents the name. */
  depth: number
  start: number
  duration: number
  /** Optional muted note after the name, such as the language and where the worker runs. */
  meta?: string
}

/**
 * The page's one trace view: a header, a time ruler, then a row per span with the name on the left, a bar on a
 * gridded track, and the duration on the right. The live demo and the Observability chapter both draw it, so a
 * trace looks the same wherever it appears.
 *
 * `elapsed` is the trace clock: each span shows the part of itself that has run by then. Bars are laid out at full
 * length and revealed with clip-path (no layout work per frame), growing linearly over `stepMs` while `running`, so
 * the trace reads as a live clock rather than in jumps.
 */
export function TraceWaterfall({
  title,
  meta,
  ticks,
  total,
  spans,
  elapsed,
  active,
  running,
  stepMs,
  compact = false,
  className,
  children,
}: {
  title: string
  meta?: ReactNode
  ticks: string[]
  /** The ruler's full width in the same unit as span start and duration. */
  total: number
  spans: readonly TraceSpan[]
  elapsed: number
  active?: string
  running: boolean
  stepMs: number
  /** Shorter name column and tighter rows, for a trace inside a narrower card. */
  compact?: boolean
  className?: string
  /** A footer row under the spans (summary, export note). */
  children?: ReactNode
}) {
  const unit = (value: number) => (total >= 1000 ? `${value}ms` : `${value} ms`)
  /* When a loop starts over the clock drops back to zero: snap the bars away instead of shrinking them backwards. */
  const previous = useRef(elapsed)
  const rewinding = elapsed < previous.current
  useEffect(() => {
    previous.current = elapsed
  }, [elapsed])
  return (
    <div className={cn(styles.trace, compact && styles.compact, className)}>
      <div className={styles.header}>
        <h3 className={styles.title}>
          <IconActivity className="size-3.5 text-muted-foreground" />
          {title}
        </h3>
        {meta ? <span className={styles.meta}>{meta}</span> : null}
      </div>
      <div className={styles.ruler} aria-hidden>
        <span className={styles.rulerSpacer} />
        <span className={styles.ticks}>
          {ticks.map((tick) => (
            <span key={tick}>{tick}</span>
          ))}
        </span>
        <span className={styles.rulerSpacer} />
      </div>
      <ol className={styles.rows}>
        {spans.map((span) => {
          const shown = Math.min(span.duration, Math.max(0, elapsed - span.start))
          const done = shown === span.duration
          const hidden = (1 - shown / span.duration) * 100
          return (
            <li key={span.id} className={styles.row} data-active={active === span.id} data-done={done}>
              <span className={styles.name} style={{ '--span-depth': span.depth } as CSSProperties}>
                {done ? (
                  <IconCheckCircle className="swap-in size-3 shrink-0 text-hero-accent" />
                ) : (
                  <IconChevronRight className="size-3 shrink-0 text-muted-foreground" />
                )}
                <span className={styles.fn}>{span.name}</span>
                {span.meta ? <span className={styles.spanMeta}>{span.meta}</span> : null}
              </span>
              <span className={styles.track} aria-hidden>
                <span
                  className={styles.bar}
                  style={{
                    left: `${(span.start / total) * 100}%`,
                    width: `${(span.duration / total) * 100}%`,
                    clipPath: `inset(0 ${hidden}% 0 0 round 2px)`,
                    transitionDuration: running && !rewinding ? `${stepMs}ms, 200ms` : '0ms',
                  }}
                />
              </span>
              <span className={styles.duration}>{shown ? unit(shown) : 'pending'}</span>
            </li>
          )
        })}
      </ol>
      {children}
    </div>
  )
}
