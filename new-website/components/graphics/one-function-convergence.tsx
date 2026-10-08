'use client'

import { type CSSProperties, useId, useLayoutEffect, useRef, useState } from 'react'

import { cn } from '@/lib/utils'
import styles from './one-function-convergence.module.css'

export type ReachPath = 'http' | 'python' | 'queue'

type Kind = 'root' | 'span' | 'wait' | 'fn'
type Span = { label: string; start: number; end: number; kind: Kind }
type Point = readonly [number, number]
type Rect = { left: number; top: number; right: number; bottom: number; cx: number; cy: number }

/** Layout anchors measured from the DOM to draw the wires. `mid` and `lane` exist only on some paths. */
const ANCHORS = ['entry', 'mid', 'engine', 'fn', 'lane'] as const
type Anchor = (typeof ANCHORS)[number]
type Geo = { w: number; h: number; vertical: boolean } & Partial<Record<Anchor, Rect>>

/** Snap a coordinate so a 1px stroke fills exactly one device row or column. */
const px = (n: number) => Math.round(n) + 0.5

/** Orthogonal polyline with rounded corners; collinear and tiny segments collapse into one. */
function route(points: readonly Point[], r = 6) {
  const pts = points.filter((p, i, all) => {
    if (i === 0) return true
    const [x0, y0] = all[i - 1]
    return Math.abs(p[0] - x0) > 0.75 || Math.abs(p[1] - y0) > 0.75
  })
  if (pts.length < 2) return ''
  let d = `M ${pts[0][0]} ${pts[0][1]}`
  for (let i = 1; i < pts.length - 1; i++) {
    const [x0, y0] = pts[i - 1]
    const [x1, y1] = pts[i]
    const [x2, y2] = pts[i + 1]
    const inLen = Math.hypot(x1 - x0, y1 - y0)
    const outLen = Math.hypot(x2 - x1, y2 - y1)
    const turn = Math.sign(x1 - x0) !== Math.sign(x2 - x1) || Math.sign(y1 - y0) !== Math.sign(y2 - y1)
    if (!turn) continue
    const rr = Math.min(r, inLen / 2, outLen / 2)
    const ix = Math.sign(x1 - x0) * rr
    const iy = Math.sign(y1 - y0) * rr
    const ox = Math.sign(x2 - x1) * rr
    const oy = Math.sign(y2 - y1) * rr
    d += ` L ${x1 - ix} ${y1 - iy} Q ${x1} ${y1} ${x1 + ox} ${y1 + oy}`
  }
  const [xl, yl] = pts[pts.length - 1]
  return `${d} L ${xl} ${yl}`
}

/** Wire from node `a` to the next node `b`: right edge to left edge, or bottom to top when stacked. */
function link(a: Rect, b: Rect, vertical: boolean): Point[] {
  if (vertical) {
    const x0 = px(a.cx)
    const x1 = px(b.cx)
    if (Math.abs(x0 - x1) < 1)
      return [
        [x0, Math.round(a.bottom)],
        [x0, Math.round(b.top)],
      ]
    const midY = px((a.bottom + b.top) / 2)
    return [
      [x0, Math.round(a.bottom)],
      [x0, midY],
      [x1, midY],
      [x1, Math.round(b.top)],
    ]
  }
  const y0 = px(a.cy)
  const y1 = px(b.cy)
  if (Math.abs(y0 - y1) < 1)
    return [
      [Math.round(a.right), y0],
      [Math.round(b.left), y0],
    ]
  const midX = px((a.right + b.left) / 2)
  return [
    [Math.round(a.right), y0],
    [midX, y0],
    [midX, y1],
    [Math.round(b.left), y1],
  ]
}

/** Small filled arrowhead pointing into the target node at the end of a wire. */
function arrowhead([x, y]: Point, vertical: boolean) {
  return vertical ? `M ${x} ${y} l -4 -6 h 8 z` : `M ${x} ${y} l -6 -4 v 8 z`
}

const reverse = (points: readonly Point[]) => [...points].reverse()

type Leg = { a: Anchor; b: Anchor; back?: boolean; delay: number; duration: number }
type Timeline = {
  legs: Leg[]
  /** When the entry node gets its result (HTTP and Python) or returns immediately (queue). */
  entryDone: number
  /** When the function starts and finishes (seconds into the run). */
  fnAt: number
  fnDone: number
  /** When the lit route fades out. */
  out: number
  traceId: string
  traceNote: string
  spans: Span[]
}

const TIMELINES: Record<ReachPath, Timeline> = {
  http: {
    legs: [
      { a: 'entry', b: 'mid', delay: 0.2, duration: 0.5 },
      { a: 'mid', b: 'engine', delay: 0.8, duration: 0.6 },
      { a: 'engine', b: 'fn', delay: 1.5, duration: 0.6 },
      { a: 'engine', b: 'fn', back: true, delay: 3.1, duration: 0.6 },
      { a: 'mid', b: 'engine', back: true, delay: 3.8, duration: 0.6 },
      { a: 'entry', b: 'mid', back: true, delay: 4.5, duration: 0.5 },
    ],
    entryDone: 5.0,
    fnAt: 2.1,
    fnDone: 3.0,
    out: 5.3,
    traceId: '4bf92f3577b34da6',
    traceNote: 'continued from traceparent',
    spans: [
      { label: 'POST /reports/generate', start: 0.2, end: 5.0, kind: 'root' },
      { label: 'api::reports::generate', start: 0.7, end: 4.5, kind: 'span' },
      { label: 'reports::generate', start: 2.1, end: 3.0, kind: 'fn' },
    ],
  },
  python: {
    legs: [
      { a: 'entry', b: 'engine', delay: 0.2, duration: 0.9 },
      { a: 'engine', b: 'fn', delay: 1.2, duration: 0.6 },
      { a: 'engine', b: 'fn', back: true, delay: 2.8, duration: 0.6 },
      { a: 'entry', b: 'engine', back: true, delay: 3.5, duration: 0.9 },
    ],
    entryDone: 4.4,
    fnAt: 1.8,
    fnDone: 2.7,
    out: 4.7,
    traceId: 'a3ce929d0e0e4736',
    traceNote: 'started by caller.py',
    spans: [
      { label: 'caller.py', start: 0.2, end: 4.4, kind: 'root' },
      { label: 'reports::generate', start: 1.8, end: 2.7, kind: 'fn' },
    ],
  },
  queue: {
    legs: [
      { a: 'entry', b: 'mid', delay: 0.2, duration: 0.5 },
      { a: 'mid', b: 'engine', delay: 2.0, duration: 0.6 },
      { a: 'engine', b: 'fn', delay: 2.7, duration: 0.6 },
    ],
    entryDone: 0.7,
    fnAt: 3.3,
    fnDone: 4.2,
    out: 4.5,
    traceId: '00f067aa0ba902b7',
    traceNote: 'started by enqueue.ts',
    spans: [
      { label: 'enqueue.ts', start: 0.2, end: 0.7, kind: 'root' },
      { label: 'queue reports', start: 0.7, end: 2.0, kind: 'wait' },
      { label: 'reports::generate', start: 3.3, end: 4.2, kind: 'fn' },
    ],
  },
}

/** Custom-property timing for the CSS animations. */
const timing = (delay: number, duration: number, extra?: Record<string, string | number>) =>
  ({ '--d': `${delay}s`, '--t': `${duration}s`, ...extra }) as CSSProperties

const mono = 'font-mono'
const muted = 'text-muted-foreground'

/** Pulses on a node each time a packet lands on it. */
function arrivals(t: Timeline, anchor: Anchor) {
  return t.legs.filter((leg) => (leg.back ? leg.a : leg.b) === anchor).map((leg) => leg.delay + leg.duration)
}

/**
 * The selected way to reach `reports::generate`, drawn as one chain: the caller, the route
 * function or queue it passes through, the engine, and the function. The engine and the function
 * never move; only the left half changes with the tab. Nodes are HTML in a grid; the wires are
 * SVG measured from the DOM and drawn at 1:1 pixels. Requests travel right in the accent colour
 * and responses travel back in the foreground colour.
 */
export function OneFunctionConvergence({
  path,
  run,
  active,
  className,
}: {
  path: ReachPath
  /** Increments each time the selected path should replay. */
  run: number
  /** Animate (in view, motion allowed). When false the diagram shows its static end state. */
  active: boolean
  className?: string
}) {
  const id = useId()
  const glowId = `${id}-glow`
  const rootRef = useRef<HTMLDivElement>(null)
  const [geo, setGeo] = useState<Geo | null>(null)
  const t = TIMELINES[path]
  const key = `${path}-${run}`

  // biome-ignore lint/correctness/useExhaustiveDependencies: The mid node mounts and unmounts with `path`, so the anchors must be re-observed.
  useLayoutEffect(() => {
    const root = rootRef.current
    if (!root) return
    const measure = () => {
      const base = root.getBoundingClientRect()
      const next: Geo = { w: base.width, h: base.height, vertical: false }
      for (const name of ANCHORS) {
        const el = root.querySelector<HTMLElement>(`[data-anchor="${name}"]`)
        if (!el) continue
        const r = el.getBoundingClientRect()
        next[name] = {
          left: r.left - base.left,
          top: r.top - base.top,
          right: r.right - base.left,
          bottom: r.bottom - base.top,
          cx: r.left - base.left + r.width / 2,
          cy: r.top - base.top + r.height / 2,
        }
      }
      if (!next.entry || !next.engine || !next.fn) return
      next.vertical = next.engine.top >= next.entry.bottom
      setGeo(next)
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(root)
    for (const el of root.querySelectorAll('[data-anchor]')) observer.observe(el)
    return () => observer.disconnect()
  }, [path])

  const ready = geo?.entry && geo.engine && geo.fn ? geo : null
  const chain: Anchor[] = path === 'python' ? ['entry', 'engine', 'fn'] : ['entry', 'mid', 'engine', 'fn']
  const wire = (a: Anchor, b: Anchor) => {
    if (!ready) return []
    const from = ready[a]
    const to = ready[b]
    return from && to ? link(from, to, ready.vertical) : []
  }
  const legs = ready
    ? t.legs.map((leg) => {
        const pts = wire(leg.a, leg.b)
        return { ...leg, d: route(leg.back ? reverse(pts) : pts) }
      })
    : []
  const laneWidth = ready?.lane ? ready.lane.right - ready.lane.left : 0

  return (
    <figure ref={rootRef} className={cn(styles.root, className)}>
      <figcaption className="sr-only">
        {path === 'http'
          ? 'An HTTP request to POST /reports/generate reaches the route function api::reports::generate, which calls reports::generate through the engine and returns the response.'
          : path === 'python'
            ? 'A Python worker calls reports::generate by name. The engine finds where it runs and returns the result.'
            : 'enqueue.ts puts a message on the reports queue and returns at once. The engine delivers it to reports::generate later.'}
      </figcaption>

      {/* Wires, drawn 1:1 in container pixels */}
      {ready ? (
        <svg
          viewBox={`0 0 ${ready.w} ${ready.h}`}
          preserveAspectRatio="none"
          aria-hidden="true"
          className={styles.wires}
        >
          <defs>
            <radialGradient id={glowId}>
              <stop offset="0" stopColor="var(--hero-accent)" stopOpacity="0.5" />
              <stop offset="1" stopColor="var(--hero-accent)" stopOpacity="0" />
            </radialGradient>
          </defs>
          <g className={styles.straight}>
            {chain.slice(0, -1).map((a, i) => {
              const pts = wire(a, chain[i + 1])
              if (pts.length < 2) return null
              return (
                <g key={a}>
                  <path
                    className={styles.wire}
                    d={route(pts)}
                    fill="none"
                    stroke="var(--line-strong)"
                    strokeDasharray={path === 'http' && a === 'entry' ? '3 4' : undefined}
                  />
                  <path d={arrowhead(pts[pts.length - 1], ready.vertical)} fill="var(--line-strong)" />
                </g>
              )
            })}
          </g>
          {/* The lit route. Keyed by run so a replay restarts; a resize only updates `d`. */}
          {active ? (
            <g key={`route-${key}`} fill="none" strokeWidth={1.75} strokeLinecap="round">
              {legs.map((leg, i) => (
                <path
                  // biome-ignore lint/suspicious/noArrayIndexKey: Legs are positional per path.
                  key={i}
                  d={leg.d}
                  pathLength={1}
                  stroke={leg.back ? 'var(--line-strong)' : 'var(--hero-accent)'}
                  className={styles.route}
                  style={timing(leg.delay, leg.duration, { '--out': `${t.out}s` })}
                />
              ))}
            </g>
          ) : null}
          {active ? (
            <g key={`packets-${key}`}>
              {legs.map((leg, i) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: Legs are positional per path.
                <g key={i}>
                  {leg.back ? null : (
                    <circle
                      r={12}
                      fill={`url(#${glowId})`}
                      className={styles.glow}
                      style={{ offsetPath: `path("${leg.d}")`, ...timing(leg.delay, leg.duration) }}
                    />
                  )}
                  <circle
                    r={leg.back ? 3.5 : 4}
                    fill={leg.back ? 'var(--foreground)' : 'var(--hero-accent)'}
                    className={styles.packet}
                    style={{ offsetPath: `path("${leg.d}")`, ...timing(leg.delay, leg.duration) }}
                  />
                </g>
              ))}
            </g>
          ) : null}
        </svg>
      ) : null}

      {/* Nodes */}
      <div className={styles.chain}>
        {path === 'http' ? (
          <Node
            anchor="entry"
            chip="http"
            label="POST /reports/generate"
            sub="traceparent: 00-4bf92f…"
            dashed
            pulses={active ? arrivals(t, 'entry') : []}
            done={t.entryDone}
            active={active}
            runKey={key}
          />
        ) : path === 'python' ? (
          <Node
            anchor="entry"
            chip="py"
            label="caller.py"
            sub="iii.trigger()"
            pulses={active ? arrivals(t, 'entry') : []}
            done={t.entryDone}
            active={active}
            runKey={key}
          />
        ) : (
          <Node
            anchor="entry"
            chip="ts"
            label="enqueue.ts"
            sub="TriggerAction.Enqueue()"
            pulses={active ? arrivals(t, 'entry') : []}
            done={t.entryDone}
            active={active}
            runKey={key}
          />
        )}

        {path === 'http' ? (
          <Node
            anchor="mid"
            chip="fn"
            label="api::reports::generate"
            sub="req → { status_code, body }"
            pulses={active ? arrivals(t, 'mid') : []}
            active={active}
            runKey={key}
          />
        ) : path === 'queue' ? (
          <Node
            anchor="mid"
            chip="q"
            label="reports"
            pulses={active ? arrivals(t, 'mid') : []}
            active={active}
            runKey={key}
          >
            {/* The message waits in the lane, then is delivered */}
            <div className="relative mt-2 h-3.5">
              <div
                data-anchor="lane"
                className={cn(styles.lane, 'absolute inset-x-0 top-1/2 border-t border-dashed')}
              />
              <span
                key={active ? `message-${key}` : 'message'}
                aria-hidden
                className={cn('absolute top-[3px] h-2 w-3.5 rounded-[3px] bg-foreground', active && styles.message)}
                style={{
                  left: active ? 0 : undefined,
                  right: active ? undefined : 0,
                  ...(active ? timing(0.8, 1.1, { '--dx': `${Math.max(laneWidth - 14, 0)}px` }) : undefined),
                }}
              />
            </div>
          </Node>
        ) : (
          <div aria-hidden className={styles.spacer} />
        )}

        {/* Engine */}
        <div className={styles.cell}>
          <div
            className={cn(styles.node, 'flex h-full items-center justify-center gap-2.5 px-3.5')}
            data-anchor="engine"
          >
            <svg viewBox="0 0 933.61 1050.31" aria-hidden="true" className="h-4 w-auto shrink-0 fill-current">
              <rect width="233.4" height="233.4" />
              <rect y="350.1" width="233.4" height="700.21" />
              <rect x="350.1" width="233.4" height="233.4" />
              <rect x="350.1" y="350.1" width="233.4" height="700.21" />
              <rect x="700.21" width="233.4" height="233.4" />
              <rect x="700.21" y="350.1" width="233.4" height="700.21" />
            </svg>
            <span className={cn(mono, 'text-[12px] uppercase tracking-[0.08em]')}>Engine</span>
            {active
              ? arrivals(t, 'engine').map((at) => (
                  <span
                    key={`${key}-${at}`}
                    aria-hidden
                    className={cn(styles.ring, styles.pulse)}
                    style={timing(at, 0.9)}
                  />
                ))
              : null}
          </div>
        </div>

        {/* The one function */}
        <Node
          anchor="fn"
          chip="fn"
          label="reports::generate"
          sub="{ team } → report"
          pulses={active ? [t.fnAt] : []}
          done={t.fnDone}
          active={active}
          runKey={key}
          halo
        />
      </div>
    </figure>
  )
}

function Node({
  anchor,
  chip,
  label,
  sub,
  dashed,
  halo,
  pulses,
  done,
  active,
  runKey,
  children,
}: {
  anchor: Anchor
  chip: string
  label: string
  sub?: string
  dashed?: boolean
  /** Hairline halo marking the one function every path ends at. */
  halo?: boolean
  /** Seconds into the run at which a packet lands here. */
  pulses: number[]
  /** Seconds into the run at which this node has its result (shows the ok dot). */
  done?: number
  active: boolean
  runKey: string
  children?: React.ReactNode
}) {
  return (
    <div className={styles.cell}>
      <div
        className={cn(styles.node, dashed && styles.dashed, 'flex h-full flex-col justify-center px-3.5 py-3')}
        data-anchor={anchor}
      >
        {halo ? <span aria-hidden className={styles.halo} /> : null}
        {pulses.map((at) => (
          <span key={`${runKey}-${at}`} aria-hidden className={cn(styles.ring, styles.pulse)} style={timing(at, 0.9)} />
        ))}
        <div className="flex items-center gap-2.5">
          <span className={styles.chip}>{chip}</span>
          <p className={cn(mono, 'min-w-0 flex-1 truncate text-[13px]')}>{label}</p>
          {done !== undefined ? (
            <span
              key={active ? `${runKey}-ok` : 'ok'}
              aria-hidden
              className={cn('size-1.5 shrink-0 rounded-full bg-ok', active && styles.pop)}
              style={active ? timing(done, 0.32) : undefined}
            />
          ) : null}
        </div>
        {sub ? <p className={cn(mono, muted, 'mt-1.5 truncate pl-[30px] text-[12px]')}>{sub}</p> : null}
        {children}
      </div>
    </div>
  )
}

/** The spans one call produces, as a small waterfall. Bars grow in step with the diagram. */
export function OneFunctionTrace({
  path,
  run,
  active,
  className,
}: {
  path: ReachPath
  run: number
  active: boolean
  className?: string
}) {
  const t = TIMELINES[path]
  const key = `${path}-${run}`
  const t0 = Math.min(...t.spans.map((s) => s.start))
  const t1 = Math.max(...t.spans.map((s) => s.end))
  const pct = (v: number) => `${(((v - t0) / (t1 - t0)) * 100).toFixed(2)}%`
  const fills: Record<Kind, string> = {
    root: 'bg-line-strong opacity-70',
    span: 'bg-line-strong',
    wait: 'bg-line',
    fn: 'bg-foreground',
  }
  return (
    <div className={cn(styles.trace, 'min-w-0 overflow-hidden rounded-[10px] border', className)}>
      <div className={cn(styles.trace, 'flex min-h-11 items-center gap-3 border-b px-4')}>
        <span className={cn(mono, muted, 'text-[12px] uppercase tracking-[0.08em]')}>Trace</span>
        <span key={path} className={cn(styles.swap, 'flex min-w-0 flex-1 items-baseline gap-3')}>
          <span className={cn(mono, 'truncate text-[13px]')}>{t.traceId}</span>
          <span className={cn(mono, muted, 'ml-auto hidden truncate text-[12px] sm:inline')}>{t.traceNote}</span>
        </span>
      </div>
      <ul key={path} className="space-y-2 px-4 py-4">
        {t.spans.map((s) => (
          <li
            key={s.label}
            className="grid items-center gap-1 sm:h-6 sm:grid-cols-[minmax(0,1.5fr)_minmax(0,2fr)] sm:gap-4"
          >
            <span className={cn(mono, 'truncate text-[13px]', s.kind === 'fn' ? 'text-foreground' : muted)}>
              {s.label}
            </span>
            <span className="relative h-2.5">
              <span
                key={active ? `${s.label}-${key}` : s.label}
                className={cn('absolute inset-y-0 rounded-[3px]', fills[s.kind], active && styles.bar)}
                style={{
                  left: pct(s.start),
                  width: `max(${pct(s.end)} - ${pct(s.start)}, 4px)`,
                  ...(active ? timing(s.start, s.end - s.start) : undefined),
                }}
              />
            </span>
          </li>
        ))}
      </ul>
    </div>
  )
}
