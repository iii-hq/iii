'use client'

import { useInView } from 'motion/react'
import { useEffect, useId, useMemo, useRef, useState } from 'react'

import { useMotionPreference } from '@/hooks/use-motion-preference'
import { type AxisFormat, axisFormats } from '@/lib/benchmarks/format'
import type { Point, Series } from '@/lib/benchmarks/views'
import { cn } from '@/lib/utils'

import { useProfile } from './profile'

export type Scale = 'linear' | 'log' | 'band'

export interface Axis {
  scale: Scale
  title: string
  format: AxisFormat
  /** Values to mark on the axis. Defaults: every data x (band, log), nice steps (linear). */
  ticks?: number[]
  /** Band axes over runs: the label of x = i (a version), in place of the format. */
  labels?: string[]
}

/** A horizontal line at `y` (a memory cap), or the diagonal y = x (everything offered arrives). */
export type Reference = { kind: 'h'; y: number; label: string; series?: string } | { kind: 'diagonal'; label: string }

const M = { top: 22, right: 74, bottom: 46, left: 58 }
const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)'

/**
 * One measurement across the machine sizes: a line per size, the selected one drawn in full with its area, the
 * others behind it for comparison. Lines are labelled where they end (a size that ran out of memory says so
 * there), so there is no legend to look up; clicking a label selects that size for the whole page. Hovering
 * scrubs a guide across the measured points and lists every size's value there. The lines draw in once, the
 * first time the chart scrolls into view; reduced motion shows them drawn.
 */
export function LineChart({
  series,
  x,
  y,
  refs = [],
  height = 300,
  label,
}: {
  series: Series[]
  x: Axis
  y: Axis
  refs?: Reference[]
  height?: number
  /** What the chart shows, for screen readers (the data follows as a table). */
  label: string
}) {
  const { selected, select } = useProfile()
  const wrap = useRef<HTMLDivElement>(null)
  const width = useWidth(wrap, 640)
  const seen = useInView(wrap, { once: true, margin: '0px 0px -15% 0px' })
  const reduce = useMotionPreference()
  const drawn = reduce || seen
  // The server renders the lines drawn (it cannot know the reader's motion preference); on the client they
  // start hidden until seen, and that change is instant: only drawing in is animated.
  const gradient = `area-${useId().replace(/[^a-zA-Z0-9_-]/g, '')}`
  const [hover, setHover] = useState<number | null>(null)

  const fx = x.labels ? (v: number) => x.labels?.[v] ?? '' : axisFormats[x.format]
  const fy = axisFormats[y.format]
  const model = useMemo(() => buildModel(series, x, y, refs, width, height), [series, x, y, refs, width, height])
  const { px, py, xs, plotW, plotH, yTicks, xTicks, lines } = model

  // Selected line last, so it draws over the others.
  const order = [...lines].sort((a, b) => Number(a.series.id === selected) - Number(b.series.id === selected))
  const labels = placeLabels(lines, selected, plotH)

  function onPointerMove(e: React.PointerEvent<SVGRectElement>) {
    const rect = e.currentTarget.getBoundingClientRect()
    const at = ((e.clientX - rect.left) / rect.width) * plotW
    let best = 0
    xs.forEach((v, i) => {
      if (Math.abs(px(v) - at) < Math.abs(px(xs[best]) - at)) best = i
    })
    setHover(best)
  }

  const hoverX = hover === null ? null : xs[hover]
  const hoverRows =
    hoverX === null
      ? []
      : lines
          .map((l) => ({ line: l, point: l.series.points.find((p) => p.x === hoverX) }))
          .filter((r): r is { line: Line; point: Point } => r.point !== undefined)
          .sort((a, b) => Number(b.line.series.id === selected) - Number(a.line.series.id === selected))

  return (
    <div ref={wrap} className="relative w-full" style={{ height }}>
      <svg
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        aria-hidden="true"
        className="block overflow-visible text-foreground"
      >
        <defs>
          <linearGradient id={gradient} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="currentColor" stopOpacity="0.14" />
            <stop offset="100%" stopColor="currentColor" stopOpacity="0" />
          </linearGradient>
        </defs>
        <g transform={`translate(${M.left},${M.top})`}>
          {/* Grid and y axis */}
          {yTicks.map((t) => (
            <g key={`y${t}`} transform={`translate(0,${py(t)})`}>
              <line x1={0} x2={plotW} className="stroke-border" strokeWidth={1} />
              <text
                x={-10}
                dy="0.32em"
                textAnchor="end"
                className="fill-muted-foreground font-mono text-[11px] tabular-nums"
              >
                {fy(t)}
              </text>
            </g>
          ))}
          <text x={-M.left + 2} y={-12} textAnchor="start" className="fill-muted-foreground text-[11px]">
            {y.title}
          </text>

          {/* x axis */}
          {xTicks.map((t) => (
            <text
              key={`x${t}`}
              x={px(t)}
              y={plotH + 18}
              textAnchor="middle"
              className="fill-muted-foreground font-mono text-[11px] tabular-nums"
            >
              {fx(t)}
            </text>
          ))}
          <text x={plotW / 2} y={plotH + 38} textAnchor="middle" className="fill-muted-foreground text-[11px]">
            {x.title}
          </text>

          {/* References */}
          {model.refLines.map((r) => (
            <g key={r.key} className="text-muted-foreground">
              <path d={r.d} fill="none" stroke="currentColor" strokeWidth={1} strokeDasharray="3 4" opacity={0.7} />
              <text x={r.labelX} y={r.labelY} textAnchor={r.anchor} className="fill-muted-foreground text-[11px]">
                {r.label}
              </text>
            </g>
          ))}

          {/* Areas and lines */}
          {order.map((l, i) => {
            const on = l.series.id === selected
            return (
              <g
                key={l.series.id}
                className={cn(
                  'transition-[color,opacity] duration-300',
                  on ? 'text-foreground' : 'text-muted-foreground',
                )}
                style={{ opacity: on ? 1 : 0.5 }}
              >
                {on && l.area ? (
                  <path
                    d={l.area}
                    fill={`url(#${gradient})`}
                    style={{
                      opacity: drawn ? 1 : 0,
                      transition: reduce || !drawn ? 'none' : `opacity 600ms ${EASE} 250ms`,
                    }}
                  />
                ) : null}
                <path
                  d={l.path}
                  pathLength={1}
                  fill="none"
                  stroke="currentColor"
                  strokeWidth={on ? 1.75 : 1.25}
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeDasharray="1 1.1"
                  style={{
                    strokeDashoffset: drawn ? 0 : 1.05,
                    transition: reduce || !drawn ? 'none' : `stroke-dashoffset 800ms ${EASE} ${i * 70}ms`,
                  }}
                />
                {on
                  ? l.series.points.map((p) => (
                      <circle
                        key={p.x}
                        cx={px(p.x)}
                        cy={py(p.y)}
                        r={2.5}
                        fill="var(--background)"
                        stroke="currentColor"
                        strokeWidth={1.5}
                        style={{
                          opacity: drawn ? 1 : 0,
                          transition: reduce || !drawn ? 'none' : `opacity 300ms ${EASE} 650ms`,
                        }}
                      />
                    ))
                  : null}
                {l.series.note && l.end ? (
                  // Where a size gave out: a cross on its last point.
                  <path
                    d={`M${l.end.x - 3.5},${l.end.y - 3.5} l7,7 M${l.end.x + 3.5},${l.end.y - 3.5} l-7,7`}
                    stroke="currentColor"
                    strokeWidth={1.5}
                    strokeLinecap="round"
                  />
                ) : null}
              </g>
            )
          })}

          {/* Hover guide and dots */}
          {hoverX !== null ? (
            <g pointerEvents="none">
              <line
                x1={px(hoverX)}
                x2={px(hoverX)}
                y1={0}
                y2={plotH}
                stroke="currentColor"
                strokeWidth={1}
                strokeDasharray="2 2"
                opacity={0.3}
              />
              {hoverRows.map(({ line, point }) => (
                <circle
                  key={line.series.id}
                  cx={px(point.x)}
                  cy={py(point.y)}
                  r={3.25}
                  fill="var(--background)"
                  stroke="currentColor"
                  strokeWidth={1.5}
                  className={line.series.id === selected ? 'text-foreground' : 'text-muted-foreground'}
                />
              ))}
            </g>
          ) : null}

          {/* Pointer target over the plot */}
          <rect
            width={plotW}
            height={plotH}
            fill="transparent"
            onPointerMove={onPointerMove}
            onPointerLeave={() => setHover(null)}
          />
        </g>
      </svg>

      {/* Line labels, as buttons: a click selects that size everywhere. */}
      {labels.map((l) => {
        const on = l.id === selected
        return (
          <button
            key={l.id}
            type="button"
            onClick={() => select(l.id)}
            className={cn(
              'absolute -translate-y-1/2 whitespace-nowrap rounded-md px-1 text-left text-[12px] leading-tight outline-none transition-colors duration-150',
              'focus-visible:outline-2 focus-visible:outline-foreground',
              on ? 'font-medium text-foreground' : 'text-muted-foreground hover:text-foreground',
            )}
            style={{ left: M.left + l.x + 6, top: M.top + l.y }}
          >
            {l.text}
            {l.note ? <span className="block font-normal text-[11px] text-muted-foreground">{l.note}</span> : null}
          </button>
        )
      })}

      {/* Tooltip */}
      {hoverX !== null && hoverRows.length ? (
        <div
          className="pointer-events-none absolute z-10 min-w-[150px] rounded-lg bg-foreground px-2.5 py-2 text-[12px] text-background shadow-lg"
          style={{
            left: M.left + px(hoverX),
            top: M.top - 6,
            translate: `${(-(hover ?? 0) / Math.max(1, xs.length - 1)) * 100}% -100%`,
          }}
        >
          <div className="mb-1 text-background/60">
            {fx(hoverX)} · {x.title.toLowerCase()}
          </div>
          {hoverRows.map(({ line, point }) => {
            const on = line.series.id === selected
            return (
              <div key={line.series.id} className={cn('tabular-nums', !on && 'text-background/60')}>
                <div className="flex justify-between gap-4">
                  <span>{line.series.label}</span>
                  <span className={on ? 'font-medium' : ''}>{fy(point.y)}</span>
                </div>
                {on && point.detail
                  ? point.detail.filter(Boolean).map((d) => (
                      <div key={d} className="text-background/60">
                        {d}
                      </div>
                    ))
                  : null}
              </div>
            )
          })}
        </div>
      ) : null}

      <DataTable label={label} series={series} x={x} y={y} />
    </div>
  )
}

/** The chart's numbers as a table, for screen readers and search engines. */
function DataTable({ label, series, x, y }: { label: string; series: Series[]; x: Axis; y: Axis }) {
  return (
    <table className="sr-only">
      <caption>{label}</caption>
      <thead>
        <tr>
          <th scope="col">Machine</th>
          <th scope="col">{x.title}</th>
          <th scope="col">{y.title}</th>
        </tr>
      </thead>
      <tbody>
        {series.flatMap((s) =>
          s.points.map((p) => (
            <tr key={`${s.id}-${p.x}`}>
              <th scope="row">{s.label}</th>
              <td>{x.labels ? x.labels[p.x] : axisFormats[x.format](p.x)}</td>
              <td>{axisFormats[y.format](p.y)}</td>
            </tr>
          )),
        )}
      </tbody>
    </table>
  )
}

interface RefLine {
  key: string
  d: string
  label: string
  labelX: number
  labelY: number
  anchor: 'start' | 'end'
}

interface Line {
  series: Series
  path: string
  area: string | null
  end: { x: number; y: number } | null
}

function buildModel(series: Series[], x: Axis, y: Axis, refs: Reference[], width: number, height: number) {
  const plotW = Math.max(120, width - M.left - M.right)
  const plotH = height - M.top - M.bottom
  const xs = [...new Set(series.flatMap((s) => s.points.map((p) => p.x)))].sort((a, b) => a - b)

  const px = xMapper(x.scale, xs, plotW)

  const ys = series.flatMap((s) => s.points.map((p) => p.y))
  const dataMax = Math.max(0, ...ys)
  // A reference only stretches the axis when it is close to the data; one far above would flatten the lines.
  const refMax = Math.max(
    0,
    ...refs.map((r) => (r.kind === 'h' ? (r.y <= dataMax * 1.6 ? r.y : 0) : Math.min(xs.at(-1) ?? 0, dataMax))),
  )
  let yMax: number
  let yTicks: number[]
  let py: (v: number) => number
  if (y.scale === 'log') {
    const positive = ys.filter((v) => v > 0)
    // A value a hair under a power of ten (999.8/s) must not open a whole decade below it.
    const lo = 10 ** Math.floor(Math.log10(Math.min(...positive, xs[0] ?? 1) * 1.05))
    yMax = Math.max(niceCeil(Math.max(dataMax, refMax) * 1.3), lo * 10)
    const span = Math.log10(yMax) - Math.log10(lo)
    py = (v: number) => plotH - ((Math.log10(Math.max(v, lo)) - Math.log10(lo)) / span) * plotH
    yTicks = y.ticks ?? Array.from({ length: Math.floor(span + 1e-9) + 1 }, (_, i) => lo * 10 ** i)
  } else {
    if (y.format === 'mib') {
      // Memory reads best on binary steps: 256 MiB, 512 MiB, 1 GiB.
      const top = Math.max(dataMax, refMax) * 1.08 || 1
      const step = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192].find((st) => top / st <= 5) ?? 16384
      yMax = Math.ceil(top / step) * step
      yTicks = y.ticks ?? Array.from({ length: yMax / step + 1 }, (_, i) => i * step)
    } else {
      yMax = niceCeil(Math.max(dataMax, refMax) * 1.08 || 1)
      yTicks = y.ticks ?? niceTicks(yMax)
    }
    py = (v: number) => plotH - (v / yMax) * plotH
  }

  const xTicks = x.ticks ?? (x.scale === 'linear' ? niceTicks(xs.at(-1) ?? 1) : xs)

  const lines: Line[] = series.map((s) => {
    const pts = s.points.map((p) => [px(p.x), py(p.y)] as const)
    const first = pts[0]
    const last = pts.at(-1)
    if (!first || !last) return { series: s, path: '', area: null, end: null }
    const path = pts.length === 1 ? `M${first[0]},${first[1]}` : monotonePath(pts)
    const area = pts.length > 1 ? `${path} L${last[0]},${plotH} L${first[0]},${plotH} Z` : null
    return { series: s, path, area, end: { x: last[0], y: last[1] } }
  })

  const refLines = refs.flatMap((r, i): RefLine[] => {
    if (r.kind === 'diagonal') {
      // y = x, sampled along the x axis so it is right on any pair of scales (a straight line only on log-log
      // or linear-linear).
      const lo = xs[0] ?? 0
      const hi = Math.min(xs.at(-1) ?? 0, yMax)
      const steps = 48
      const at = (t: number) => (x.scale === 'log' ? lo * (hi / lo) ** t : lo + (hi - lo) * t)
      const d = Array.from({ length: steps + 1 }, (_, k) => {
        const v = at(k / steps)
        return `${k ? 'L' : 'M'}${px(v).toFixed(1)},${py(v).toFixed(1)}`
      }).join(' ')
      return [{ key: `d${i}`, d, label: r.label, labelX: px(hi) - 4, labelY: py(hi) - 8, anchor: 'end' }]
    }
    if (r.y > yMax) return []
    return [
      {
        key: `h${i}`,
        d: `M0,${py(r.y)} H${plotW}`,
        label: r.label,
        labelX: 4,
        labelY: py(r.y) - 6,
        anchor: 'start',
      },
    ]
  })

  return { px, py, xs, plotW, plotH, yTicks, xTicks, lines, refLines }
}

function xMapper(scale: Scale, xs: number[], plotW: number) {
  if (xs.length === 0) return () => 0
  if (scale === 'band') {
    const step = xs.length > 1 ? plotW / (xs.length - 1) : 0
    return (v: number) => {
      const i = xs.indexOf(v)
      return xs.length > 1 ? i * step : plotW / 2
    }
  }
  const top = xs[xs.length - 1]
  if (scale === 'log') {
    const lo = Math.log10(Math.max(xs[0], 1e-9))
    const hi = Math.log10(Math.max(top, 1e-9))
    const span = hi - lo || 1
    return (v: number) => ((Math.log10(Math.max(v, 1e-9)) - lo) / span) * plotW
  }
  const hi = top || 1
  return (v: number) => (v / hi) * plotW
}

/** Labels at each line's end, pushed apart so they never sit on each other. */
function placeLabels(lines: Line[], selected: string, plotH: number) {
  const labels = lines
    .flatMap((l) =>
      l.end
        ? [
            {
              id: l.series.id,
              text: l.series.label,
              note: l.series.note,
              x: l.end.x,
              y: l.end.y,
              on: l.series.id === selected,
            },
          ]
        : [],
    )
    .sort((a, b) => a.y - b.y)
  const gap = 16
  for (let i = 1; i < labels.length; i++) {
    const prev = labels[i - 1]
    const need = prev.y + gap + (prev.note ? 12 : 0)
    if (labels[i].y < need) labels[i].y = need
  }
  // If the stack ran off the bottom, slide it back up.
  const overflow = (labels.at(-1)?.y ?? 0) - plotH
  if (overflow > 0) for (const l of labels) l.y -= overflow
  return labels
}

function niceCeil(v: number): number {
  const exp = 10 ** Math.floor(Math.log10(v))
  const f = v / exp
  const nice = [1, 1.2, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10].find((n) => f <= n) ?? 10
  return nice * exp
}

function niceTicks(max: number, count = 4): number[] {
  const step = niceCeil(max / count)
  const ticks: number[] = []
  for (let v = 0; v <= max + step * 1e-9; v += step) ticks.push(Number(v.toPrecision(12)))
  return ticks
}

/** Fritsch–Carlson monotone cubic: smooth, never overshooting a measured value (the registry charts' curve). */
function monotonePath(points: ReadonlyArray<readonly [number, number]>): string {
  const n = points.length
  const slopes = points.slice(0, -1).map(([x, y], i) => (points[i + 1][1] - y) / (points[i + 1][0] - x || 1e-9))
  const tangents = points.map((_, i) => {
    if (i === 0) return slopes[0]
    if (i === n - 1) return slopes[n - 2]
    return slopes[i - 1] * slopes[i] <= 0 ? 0 : (slopes[i - 1] + slopes[i]) / 2
  })
  for (let i = 0; i < n - 1; i++) {
    if (slopes[i] === 0) {
      tangents[i] = tangents[i + 1] = 0
      continue
    }
    const a = tangents[i] / slopes[i]
    const b = tangents[i + 1] / slopes[i]
    const h = a * a + b * b
    if (h > 9) {
      const k = 3 / Math.sqrt(h)
      tangents[i] = k * a * slopes[i]
      tangents[i + 1] = k * b * slopes[i]
    }
  }
  let d = `M${points[0][0].toFixed(2)},${points[0][1].toFixed(2)}`
  for (let i = 0; i < n - 1; i++) {
    const [x0, y0] = points[i]
    const [x1, y1] = points[i + 1]
    const dx = (x1 - x0) / 3
    d += ` C${(x0 + dx).toFixed(2)},${(y0 + tangents[i] * dx).toFixed(2)} ${(x1 - dx).toFixed(2)},${(y1 - tangents[i + 1] * dx).toFixed(2)} ${x1.toFixed(2)},${y1.toFixed(2)}`
  }
  return d
}

/** The element's width, following resizes; `fallback` until it is measured (and on the server). */
function useWidth(ref: React.RefObject<HTMLElement | null>, fallback: number): number {
  const [width, setWidth] = useState(fallback)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const observer = new ResizeObserver(([entry]) => setWidth(Math.round(entry.contentRect.width)))
    observer.observe(el)
    setWidth(Math.round(el.getBoundingClientRect().width))
    return () => observer.disconnect()
  }, [ref])
  return width
}
