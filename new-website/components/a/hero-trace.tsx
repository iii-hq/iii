'use client'

import { animate, motion, useMotionValue, useTransform } from 'motion/react'
import { useEffect, useState } from 'react'

import { useGraphicLoop } from '@/components/graphics/use-graphic-loop'
import { IconArrowDown, IconCategory, IconLayers, IconTimeCircle } from '@/components/icons/iconly'
import { easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

import { hero } from './content'
import { KindGlyph, TraceIcon, TraceMinimap } from './trace-glyphs'

/** One loop: the trace draws, holds complete, then starts again. */
const LOOP_S = 6.6
/** Seconds the whole trace takes to draw; every delay and width below is a fraction of it. */
const DRAW_S = 3.2
/** Wall-clock length of the trace itself. Span widths are percentages of this. */
const TRACE_MS = 2000
/** Ruler marks, as the console draws them: quarters of the total. */
const RULER = [0, 25, 50, 75, 100]
const TRACE_ID = '7f3a2c9e'

/** The console's duration format: whole milliseconds below a second, two decimals above. */
function formatDuration(ms: number) {
  return ms < 1000 ? `${Math.round(ms)}ms` : `${(ms / 1000).toFixed(2)}s`
}

function percentile(values: number[], p: number) {
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.max(0, Math.ceil((p / 100) * sorted.length) - 1)]
}

type SpanState = 'pending' | 'running' | 'done'

const SPANS = hero.spans.map((span) => ({
  ...span,
  worker: span.fn.split('::')[0],
  ms: span.length * (TRACE_MS / 100),
  startS: (span.start / 100) * DRAW_S,
  drawS: Math.max(0.3, (span.length / 100) * DRAW_S),
}))
const TOTAL_MS = SPANS.reduce((sum, span) => sum + span.ms, 0)
const P = {
  p50: percentile(
    SPANS.map((s) => s.ms),
    50,
  ),
  p95: percentile(
    SPANS.map((s) => s.ms),
    95,
  ),
  p99: percentile(
    SPANS.map((s) => s.ms),
    99,
  ),
}

/** Tones for the per-worker share bar: the root in the accent, children stepping down in strength. */
const TONES = [
  'bg-hero-accent',
  'bg-foreground/85',
  'bg-foreground/70',
  'bg-foreground/55',
  'bg-foreground/42',
  'bg-foreground/30',
]

/**
 * Ticks ten times a second while the figure is on screen. `t` is seconds into the current loop; off screen (or
 * with reduced motion) the clock sits past the end so the trace renders complete.
 */
function useTraceClock(active: boolean) {
  const [clock, setClock] = useState({ t: 0, cycle: 0 })
  useEffect(() => {
    if (!active) return
    const started = performance.now()
    setClock({ t: 0, cycle: 0 })
    const id = window.setInterval(() => {
      const elapsed = (performance.now() - started) / 1000
      const cycle = Math.floor(elapsed / LOOP_S)
      setClock({ t: elapsed - cycle * LOOP_S, cycle })
    }, 100)
    return () => window.clearInterval(id)
  }, [active])
  return active ? clock : { t: Number.POSITIVE_INFINITY, cycle: 0 }
}

function spanState(t: number, startS: number, drawS: number): SpanState {
  if (t < startS) return 'pending'
  if (t < startS + drawS) return 'running'
  return 'done'
}

/** The trace's total duration counting up as the root span runs, rendered straight from a motion value. */
function DurationCounter({ active, cycle }: { active: boolean; cycle: number }) {
  const ms = useMotionValue(TRACE_MS)
  const label = useTransform(ms, (v) => formatDuration(v))
  // biome-ignore lint/correctness/useExhaustiveDependencies: Each loop restarts the count from zero.
  useEffect(() => {
    if (!active) {
      ms.set(TRACE_MS)
      return
    }
    ms.set(0)
    const controls = animate(ms, TRACE_MS, { duration: DRAW_S, ease: 'linear' })
    return () => controls.stop()
  }, [active, cycle, ms])
  return <motion.span className="tabular-nums">{label}</motion.span>
}

function Chip({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <span
      className={cn(
        'inline-flex h-6 items-center gap-1.5 rounded-md border bg-faint px-2 font-sans text-[11.5px] text-muted-foreground tabular-nums leading-none',
        className,
      )}
    >
      {children}
    </span>
  )
}

/** Quarter grid lines inside a bar track, the way the console rules its waterfall rows. */
function TrackGrid() {
  return (
    <>
      {RULER.map((p) => (
        <span
          key={p}
          aria-hidden
          className={cn('absolute inset-y-0 w-px', p === 0 ? 'bg-line-strong' : 'bg-line/70')}
          style={{ left: `${p}%` }}
        />
      ))}
    </>
  )
}

/**
 * The hero's trace view, drawn after the console's trace panel: a header with the root span and its stats, the
 * per-worker share bar, the waterfall itself, and the workers footer. One request, six spans, drawing themselves in
 * order on a loop, so the first thing on the page is execution made visible.
 */
export function HeroTrace({ className }: { className?: string }) {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const { t, cycle } = useTraceClock(active)
  const states = SPANS.map((span) => spanState(t, span.startS, span.drawS))
  const started = states.filter((s) => s !== 'pending').length
  const complete = states.every((s) => s === 'done')
  const runKey = `${active ? 'live' : 'still'}-${cycle}`

  return (
    <figure
      ref={ref}
      className={cn(
        'relative min-w-0 overflow-hidden rounded-xl border bg-card shadow-[0_8px_30px_-12px_var(--line)] @container',
        className,
      )}
    >
      <figcaption className="sr-only">
        A trace of one request through iii, shown the way the console shows it: an agent run that pushes to a queue,
        writes to the database, reacts to a state change, runs tests and deploys. Each span is a function on a different
        worker.
      </figcaption>

      {/* Header: root span, trace id, stats. */}
      <div className="border-b px-4 pt-3.5 pb-3 @min-[680px]:px-5">
        <div className="flex items-center gap-2.5">
          <TraceIcon className="shrink-0 text-muted-foreground" />
          <span className="hidden font-sans text-[11px] text-muted-foreground uppercase tracking-[0.12em] @min-[560px]:inline">
            Traces
          </span>
          <span className="flex items-center gap-1.5 font-sans text-[13px] text-muted-foreground">
            <span className="relative flex size-1.5">
              {!complete ? (
                <motion.span
                  aria-hidden
                  className="absolute inset-0 rounded-full bg-ok"
                  animate={{ scale: [1, 2.4], opacity: [0.6, 0] }}
                  transition={{ duration: 1.2, repeat: Number.POSITIVE_INFINITY, ease: 'easeOut' }}
                />
              ) : null}
              <span className="relative size-1.5 rounded-full bg-ok" />
            </span>
            {complete ? 'Connected to iii' : 'Live'}
          </span>
          <TraceMinimap
            key={runKey}
            spans={SPANS}
            active={active}
            drawSeconds={DRAW_S}
            className="ml-auto hidden shrink-0 @min-[560px]:block"
          />
        </div>
        <div className="mt-3 flex min-w-0 items-center gap-2.5">
          <span className="inline-flex h-5 shrink-0 items-center rounded border border-hero-accent/30 bg-hero-accent/8 px-1.5 font-sans font-medium text-[10px] text-hero-accent uppercase tracking-[0.08em] leading-none">
            harness
          </span>
          <span className="truncate font-mono text-[15px] text-foreground @min-[680px]:text-base">
            <span className="text-muted-foreground">execute </span>agent::run
          </span>
        </div>
        <div className="mt-2.5 flex flex-wrap items-center gap-x-2.5 gap-y-2">
          <span className="font-mono text-[11.5px] text-muted-foreground tabular-nums">{TRACE_ID}</span>
          <span aria-hidden className="h-3.5 w-px bg-border" />
          <Chip className={cn(complete && 'border-hero-accent/30 text-hero-accent')}>
            <IconTimeCircle className="size-3" />
            <DurationCounter active={active} cycle={cycle} />
          </Chip>
          <Chip>
            <IconLayers className="size-3" />
            {SPANS.length} spans
          </Chip>
          <Chip className="hidden @min-[480px]:inline-flex">
            <IconCategory className="size-3" />
            {SPANS.length} workers
          </Chip>
        </div>

        {/* Per-worker share of the trace, filling in as each span finishes. */}
        <div className="mt-3.5">
          <div className="flex h-1 gap-px overflow-hidden rounded-full bg-faint">
            {SPANS.map((span, i) => (
              <span key={span.id} className="relative h-full" style={{ width: `${(span.ms / TOTAL_MS) * 100}%` }}>
                <motion.span
                  aria-hidden
                  className={cn('absolute inset-0 origin-left rounded-full', TONES[i])}
                  initial={false}
                  animate={{ scaleX: states[i] === 'done' ? 1 : 0 }}
                  transition={{ duration: 0.4, ease: easeOut }}
                />
              </span>
            ))}
          </div>
          <ul className="mt-2 flex flex-wrap gap-x-3.5 gap-y-1 font-mono text-[11px] text-muted-foreground">
            {SPANS.map((span, i) => (
              <li
                key={span.id}
                className={cn(
                  'flex items-center gap-1.5 transition-opacity duration-300',
                  states[i] === 'pending' ? 'opacity-40' : 'opacity-100',
                )}
              >
                <span aria-hidden className={cn('size-1.5 rounded-[2px]', TONES[i])} />
                {span.worker}
              </li>
            ))}
          </ul>
        </div>
      </div>

      {/* Toolbar: view tabs, as in the console, with the waterfall selected. */}
      <div className="flex h-10 items-center justify-between border-b px-4 font-sans text-[12.5px] @min-[680px]:px-5">
        <div className="flex h-full items-end gap-4">
          <span className="flex h-full items-center text-muted-foreground">timeline</span>
          <span className="relative flex h-full items-center text-foreground">
            waterfall
            <span aria-hidden className="absolute inset-x-0 -bottom-px h-px bg-foreground" />
          </span>
        </div>
        <span className="text-muted-foreground tabular-nums">
          {started} of {SPANS.length} spans
        </span>
      </div>

      {/* Waterfall. */}
      <div className="relative px-4 pt-3 pb-4 @min-[680px]:px-5">
        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 bg-dots opacity-40 [mask-image:radial-gradient(ellipse_at_center,black,transparent_78%)]"
        />
        <div className="relative grid grid-cols-[minmax(0,1fr)] gap-y-1 @min-[560px]:grid-cols-[232px_minmax(0,1fr)] @min-[560px]:gap-x-5 @min-[900px]:grid-cols-[348px_minmax(0,1fr)]">
          <div className="hidden h-5 items-center font-sans text-[10px] text-muted-foreground uppercase tracking-[0.12em] @min-[560px]:flex">
            Span
          </div>
          <div className="relative mr-14 hidden h-5 @min-[560px]:block">
            {RULER.map((p, i) => (
              <span
                key={p}
                className={cn(
                  'absolute top-0 font-sans text-[10px] text-muted-foreground tabular-nums',
                  i === 0 ? '' : i === RULER.length - 1 ? '-translate-x-full' : '-translate-x-1/2',
                )}
                style={{ left: `${p}%` }}
              >
                {formatDuration((TRACE_MS * p) / 100)}
              </span>
            ))}
            <span aria-hidden className="absolute inset-x-0 bottom-0 h-px bg-border" />
          </div>

          {SPANS.map((span, i) => {
            const state = states[i]
            const delay = active ? span.startS : 0
            const duration = active ? span.drawS : 0
            return (
              <div key={`${runKey}-${span.id}`} className="contents">
                {/* Label cell */}
                <motion.div
                  initial={false}
                  animate={{ opacity: state === 'pending' ? 0.3 : 1, x: state === 'pending' ? -4 : 0 }}
                  transition={{ duration: 0.3, ease: easeOut }}
                  className={cn(
                    'flex h-7 min-w-0 items-center gap-2 rounded-md pr-2 font-sans text-[13px] transition-colors duration-300',
                    state === 'running' ? 'bg-faint' : '',
                  )}
                >
                  {span.depth ? (
                    <span aria-hidden className="ml-2 h-7 w-4 shrink-0 border-l border-line" />
                  ) : (
                    <IconArrowDown className="ml-1 size-3 shrink-0 text-muted-foreground" />
                  )}
                  <span
                    aria-hidden
                    className={cn(
                      'size-2 shrink-0 rounded-[2px] transition-colors duration-300',
                      state === 'done'
                        ? 'bg-ok'
                        : state === 'running'
                          ? 'bg-muted-foreground'
                          : 'border border-line-strong',
                    )}
                  />
                  <span
                    className={cn(
                      'flex size-5 shrink-0 items-center justify-center rounded border bg-faint transition-colors duration-300',
                      state === 'pending' ? 'text-muted-foreground' : 'text-foreground',
                    )}
                  >
                    <KindGlyph kind={span.kind} />
                  </span>
                  <span className="truncate text-foreground">{span.label}</span>
                  <span className="hidden truncate font-mono text-[12.5px] text-muted-foreground @min-[900px]:inline">
                    {span.fn}
                  </span>
                  <motion.span
                    className="ml-auto shrink-0 pl-2 text-[11px] text-muted-foreground tabular-nums"
                    initial={false}
                    animate={{ opacity: state === 'done' ? 1 : 0 }}
                    transition={{ duration: 0.3, ease: easeOut }}
                  >
                    {formatDuration(span.ms)}
                  </motion.span>
                </motion.div>

                {/* Bar cell */}
                <div className="relative mr-14 h-7 min-w-0">
                  <TrackGrid />
                  <motion.span
                    aria-hidden
                    className={cn(
                      'absolute top-1/2 h-3 -translate-y-1/2 rounded-[3px]',
                      span.depth ? 'bg-foreground/70' : 'bg-hero-accent',
                      state === 'running' && span.depth ? 'shadow-[0_0_0_2px_var(--faint)]' : '',
                    )}
                    style={{
                      left: `${span.start}%`,
                      width: `${span.length}%`,
                      minWidth: 3,
                      transformOrigin: 'left center',
                    }}
                    initial={active ? { scaleX: 0, opacity: 0 } : false}
                    animate={{ scaleX: 1, opacity: 1 }}
                    transition={{
                      scaleX: { duration, delay, ease: 'linear' },
                      opacity: { duration: 0.2, delay },
                    }}
                  />
                </div>
              </div>
            )
          })}
        </div>
      </div>

      {/* Footer: the console's workers strip with its percentiles. */}
      <div className="flex h-10 items-center justify-between gap-3 border-t bg-faint/60 px-4 font-sans text-[11px] text-muted-foreground @min-[680px]:px-5">
        <span className="flex items-center gap-2 uppercase tracking-[0.12em]">
          <IconArrowDown className="size-3 -rotate-90" />
          Workers
          <span className="hidden font-mono normal-case tracking-normal @min-[480px]:inline">
            · agent → queue → pg → state → ci → deploy
          </span>
        </span>
        <motion.span
          className="flex shrink-0 items-center gap-3 tabular-nums"
          initial={false}
          animate={{ opacity: complete ? 1 : 0.35 }}
          transition={{ duration: 0.3, ease: easeOut }}
        >
          <span>
            p50 <span className="text-foreground">{formatDuration(P.p50)}</span>
          </span>
          <span className="hidden @min-[560px]:inline">
            p95 <span className="text-foreground">{formatDuration(P.p95)}</span>
          </span>
          <span className="hidden @min-[560px]:inline">
            p99 <span className="text-foreground">{formatDuration(P.p99)}</span>
          </span>
        </motion.span>
      </div>
    </figure>
  )
}
