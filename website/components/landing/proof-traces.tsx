'use client'

import { ChevronRightIcon, SquareFunctionIcon } from 'lucide-react'
import { type CSSProperties, useEffect, useRef, useState } from 'react'

import { cn } from '@/lib/utils'
import { percentile, SPANS, TURN_MS, WORKER_COLORS } from './proof-session'
import styles from './proof-traces.module.css'
import { TraceWaterfall } from './trace-waterfall'

/**
 * The inspector: ADE's live traces pane, redrawn quieter. A live strip with the trace's minimap arriving at the right
 * edge, one row of numbers (the two the section's copy claims, discovery and context, beside the turn time), a
 * Timeline / Waterfall switch, and the Workers summary. Timeline is ADE's own view: one bar per span coloured by
 * worker, tree connectors from parent to child, the name inside the bar when it fits. Waterfall is the page's shared
 * trace view, so the same run reads the same way it does in the Live demo.
 */

const ROW = 30
const TOTAL = 4800
const AXIS = ['0', '1.2s', '2.4s', '3.6s', '4.8s']
const CLOCK = ['09:41:00', '09:41:15', '09:41:30', '09:41:45']
const ROW_OF = Object.fromEntries(SPANS.map((span, i) => [span.id, i]))
const WORKERS = new Set(SPANS.map((span) => span.worker)).size

type View = 'timeline' | 'waterfall'

const pct = (ms: number) => `${(ms / TOTAL) * 100}%`
const seconds = (ms: number) => (ms >= 1000 ? `${(ms / 1000).toFixed(2)}s` : `${ms}ms`)

export function ProofTraces({
  elapsed,
  active,
  running,
  stepMs,
  discovery,
  context,
}: {
  elapsed: number
  active?: string
  running: boolean
  stepMs: number
  /** Discovery time in ms once the search has returned. */
  discovery?: number
  /** Context window in thousands of tokens. */
  context: number
}) {
  const [view, setView] = useState<View>('timeline')
  /* Bars grow linearly with the trace clock; when the loop starts over they snap away instead of shrinking back. */
  const previous = useRef(elapsed)
  const rewinding = elapsed < previous.current
  useEffect(() => {
    previous.current = elapsed
  }, [elapsed])
  const grow = running && !rewinding ? `${stepMs}ms` : '0ms'

  const started = SPANS.filter((span) => elapsed > span.start || (span.start === 0 && elapsed > 0))
  const finished = SPANS.filter((span) => elapsed >= span.start + span.duration).map((span) => span.duration)
  const live = started.length > 0 && finished.length < SPANS.length

  return (
    <div className={styles.pane}>
      {/* Header: the trace, whether it is still running, and how many spans have arrived. */}
      <div className={styles.header}>
        <span className={styles.title}>Traces</span>
        <code className={styles.traceId}>trace-fix-staging-checkout</code>
        <span className={styles.spanCount}>
          <span aria-hidden className={styles.liveDot} data-live={live} />
          <span key={started.length} className="swap-in">
            {started.length}
          </span>{' '}
          spans
        </span>
      </div>

      {/* Live strip: wall-clock gridlines, with this trace's minimap drawing in at the right edge. */}
      <div className={styles.strip} aria-hidden>
        {CLOCK.map((time, i) => (
          <span key={time} className={styles.clock} style={{ left: `${(i / CLOCK.length) * 100}%` }}>
            {time}
          </span>
        ))}
        <span className={styles.minimap}>
          {SPANS.map((span, i) => (
            <span
              key={span.id}
              className={styles.miniBar}
              style={{
                top: i * 4,
                left: pct(span.start),
                width: pct(Math.max(span.duration, 60)),
                background: WORKER_COLORS[span.worker],
                clipPath: `inset(0 ${(1 - shownShare(span, elapsed)) * 100}% 0 0)`,
                transitionDuration: grow,
              }}
            />
          ))}
        </span>
      </div>

      {/* The numbers: the turn so far, and the two claims from the copy. */}
      <div className={styles.stats}>
        <Stat label="Turn" value={elapsed > 0 ? seconds(Math.min(elapsed, TURN_MS)) : '–'} on={elapsed > 0} />
        <Stat label="Discovery" value={discovery != null ? `${discovery} ms` : '–'} on={discovery != null} />
        <Stat label="Context" value={`${context.toFixed(1)}k tokens`} on />
      </div>

      {/* Views */}
      <div className={styles.views}>
        <div className={styles.switch} role="tablist" aria-label="Trace view" data-view={view}>
          <span aria-hidden className={styles.switchPill} />
          {(['timeline', 'waterfall'] as const).map((option) => (
            <button
              key={option}
              type="button"
              role="tab"
              aria-selected={view === option}
              onClick={() => setView(option)}
              className={styles.switchOption}
            >
              {option === 'timeline' ? 'Timeline' : 'Waterfall'}
            </button>
          ))}
        </div>
        <span className={styles.viewNote}>{view === 'timeline' ? 'Coloured by worker' : 'Span by span'}</span>
      </div>

      <div className={styles.body}>
        {view === 'timeline' ? (
          <div key="timeline" className={cn(styles.timeline, 'swap-in')}>
            <div className={styles.axis} aria-hidden>
              {AXIS.map((tick) => (
                <span key={tick}>{tick}</span>
              ))}
            </div>
            <ol className={styles.rows} style={{ height: SPANS.length * ROW }}>
              {SPANS.map((span, i) => {
                const share = shownShare(span, elapsed)
                const on = started.includes(span)
                const color = WORKER_COLORS[span.worker]
                const parentRow = span.parent ? ROW_OF[span.parent] : undefined
                const parent = parentRow != null ? SPANS[parentRow] : undefined
                return (
                  <li
                    key={span.id}
                    className={styles.row}
                    data-on={on}
                    data-active={active === span.id}
                    style={{ top: i * ROW, '--bar': color } as CSSProperties}
                  >
                    {parent && parentRow != null ? (
                      <span
                        aria-hidden
                        className={styles.connector}
                        style={{
                          left: pct(parent.start),
                          width: `calc(${pct(span.start - parent.start)} + 1px)`,
                          height: (i - parentRow) * ROW,
                        }}
                      />
                    ) : null}
                    <span
                      className={styles.bar}
                      data-label={span.label}
                      style={{
                        left: pct(span.start),
                        width: `max(${pct(span.duration)}, 20px)`,
                        clipPath: `inset(-1px ${(1 - share) * 100}% -1px 0 round 5px)`,
                        transitionDuration: grow,
                      }}
                    >
                      <SquareFunctionIcon aria-hidden strokeWidth={2} className={styles.fnIcon} />
                      {span.label === 'inside' ? <span className={styles.barName}>{span.name}</span> : null}
                    </span>
                    {span.label !== 'inside' ? (
                      <span
                        className={styles.outsideName}
                        data-side={span.label}
                        style={
                          span.label === 'right'
                            ? { left: `calc(${pct(span.start + span.duration)} + 8px)` }
                            : { right: `calc(${pct(TOTAL - span.start)} + 8px)` }
                        }
                      >
                        {span.name}
                      </span>
                    ) : null}
                  </li>
                )
              })}
            </ol>
          </div>
        ) : (
          <div key="waterfall" className="swap-in">
            <TraceWaterfall
              compact
              className={styles.waterfall}
              title="harness::turn"
              ticks={AXIS}
              total={TOTAL}
              spans={SPANS}
              elapsed={elapsed}
              active={active}
              running={running}
              stepMs={stepMs}
            />
          </div>
        )}
      </div>

      {/* Workers: how many took part and the latency spread of the spans so far. */}
      <div className={styles.footer}>
        <span className={styles.workers}>
          <ChevronRightIcon aria-hidden strokeWidth={1.75} className="size-3.5" />
          Workers <span className={styles.workerCount}>{WORKERS}</span>
        </span>
        <span className={styles.percentiles}>
          <span>
            p50 <b>{finished.length ? seconds(percentile(finished, 50)) : '–'}</b>
          </span>
          <span>
            p95 <b>{finished.length ? seconds(percentile(finished, 95)) : '–'}</b>
          </span>
          <span>
            p99 <b>{finished.length ? seconds(percentile(finished, 99)) : '–'}</b>
          </span>
        </span>
      </div>
    </div>
  )
}

function shownShare(span: (typeof SPANS)[number], elapsed: number) {
  return Math.min(1, Math.max(0, (elapsed - span.start) / span.duration))
}

function Stat({ label, value, on }: { label: string; value: string; on: boolean }) {
  return (
    <div className={styles.stat} data-on={on}>
      <span>{label}</span>
      <strong key={value} className="swap-in">
        {value}
      </strong>
    </div>
  )
}
