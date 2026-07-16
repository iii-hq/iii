import { cn } from '@lib/lib/utils'
import { useMemo } from 'react'

/**
 * archetype A8 — the "many paths converge to one" funnel.
 *
 * use when the spec collapses many cases into a single guaranteed
 * mechanism: many spawn paths -> one owned-spawn, many inputs -> one
 * reaper. the input boxes sit across the top and every arrow converges
 * down into one accent target box. an optional `reject` draws a dashed
 * alert path off to the side, marked removed/forbidden with an x.
 *
 * mostly static; a subtle travelling pulse rides the convergence edges,
 * gated behind prefers-reduced-motion.
 */

export interface FunnelPath {
  id: string
  label: string
  desc?: string
}

interface FunnelProps {
  title?: string
  /** 2-5 input boxes across the top */
  paths: FunnelPath[]
  /** the single convergence box at the bottom */
  target: { label: string; sub?: string }
  /** optional dashed alert path that is eliminated */
  reject?: { label: string; desc?: string }
  className?: string
}

// drawing geometry, in viewBox units
const W = 980
const PATH_Y = 22
const PATH_H = 64
const PATH_W = 188
const PATH_GAP = 18
const TARGET_H = 86
const TARGET_W = 360
const TARGET_Y = 232
const REJECT_W = 210
const REJECT_H = 64
const REJECT_Y = 243

export function Funnel({ title = 'many paths converge to one', paths, target, reject, className }: FunnelProps) {
  const reducedMotion = useMemo(
    () => typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    [],
  )

  // the input boxes are centered as a block within the left region so the
  // convergence stays visually balanced even when a reject column is shown.
  const n = paths.length
  const blockW = n * PATH_W + (n - 1) * PATH_GAP
  // when there is a reject path, hold the left region narrower to leave room
  const regionRight = reject ? W - REJECT_W - 56 : W
  const blockX = Math.max(20, (regionRight - blockW) / 2)
  const pathX = (i: number) => blockX + i * (PATH_W + PATH_GAP)

  const targetX = (regionRight - TARGET_W) / 2 < 20 ? 20 : (regionRight - TARGET_W) / 2
  const targetCx = targetX + TARGET_W / 2
  const targetTop = TARGET_Y

  const rejectX = W - REJECT_W - 20
  const rejectCx = rejectX + REJECT_W / 2

  const height = 340

  return (
    <div className={cn('border border-rule bg-bg', className)}>
      <div className="flex items-center justify-between bg-panel px-3.5 py-2 border-b border-rule">
        <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">{title}</span>
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost tabular-nums">
          {n} paths {'→'} 1
        </span>
      </div>

      <div className="overflow-x-auto p-3">
        <svg
          viewBox={`0 0 ${W} ${height}`}
          className="w-full h-auto min-w-[680px] font-mono select-none"
          role="img"
          aria-label={`${n} input paths converging into one mechanism: ${target.label}${reject ? `, with the eliminated path ${reject.label}` : ''}`}
        >
          <defs>
            <marker
              id="fun-arr"
              viewBox="0 0 8 8"
              refX="7"
              refY="4"
              markerWidth="6.5"
              markerHeight="6.5"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-ink-faint" />
            </marker>
            <marker
              id="fun-arr-accent"
              viewBox="0 0 8 8"
              refX="7"
              refY="4"
              markerWidth="6.5"
              markerHeight="6.5"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-accent" />
            </marker>
            <marker
              id="fun-arr-alert"
              viewBox="0 0 8 8"
              refX="7"
              refY="4"
              markerWidth="6.5"
              markerHeight="6.5"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-alert" />
            </marker>
          </defs>

          {/* converging edges: each input drops, curves toward the target top */}
          {paths.map((p, i) => {
            const startX = pathX(i) + PATH_W / 2
            const startY = PATH_Y + PATH_H
            const endY = targetTop
            const d = `M ${startX} ${startY} C ${startX} ${startY + 56}, ${targetCx} ${endY - 56}, ${targetCx} ${endY}`
            return (
              <g key={`edge-${p.id}`}>
                <path d={d} fill="none" strokeWidth={1.2} markerEnd="url(#fun-arr-accent)" className="stroke-accent" />
                {!reducedMotion ? (
                  <circle r="2.4" className="fill-accent">
                    <animateMotion dur="2s" begin={`${i * 0.32}s`} repeatCount="indefinite" path={d} />
                  </circle>
                ) : null}
              </g>
            )
          })}

          {/* rejected edge: dashed, alert, struck out with an x */}
          {reject
            ? (() => {
                const startX = rejectCx
                const startY = PATH_Y + PATH_H
                const endY = REJECT_Y
                const d = `M ${startX} ${startY} L ${startX} ${endY}`
                const midY = (startY + endY) / 2
                return (
                  <g>
                    <path
                      d={d}
                      fill="none"
                      strokeWidth={1.2}
                      strokeDasharray="5 4"
                      markerEnd="url(#fun-arr-alert)"
                      className="stroke-alert"
                    />
                    {/* x marking the path as removed */}
                    <g className="stroke-alert" strokeWidth={1.6} strokeLinecap="round">
                      <line x1={startX - 7} y1={midY - 7} x2={startX + 7} y2={midY + 7} />
                      <line x1={startX - 7} y1={midY + 7} x2={startX + 7} y2={midY - 7} />
                    </g>
                    <rect
                      x={startX - 11}
                      y={midY - 11}
                      width={22}
                      height={22}
                      rx={2}
                      className="fill-bg stroke-alert"
                      strokeWidth={1}
                      style={{ fillOpacity: 0.0 }}
                    />
                  </g>
                )
              })()
            : null}

          {/* input path boxes across the top */}
          {paths.map((p, i) => {
            const x = pathX(i)
            return (
              <g key={`box-${p.id}`}>
                <rect x={x} y={PATH_Y} width={PATH_W} height={PATH_H} className="fill-bg stroke-rule" strokeWidth={1} />
                <text x={x + 13} y={PATH_Y + 26} fontSize="12" fontWeight={600} className="fill-ink">
                  {p.label}
                </text>
                {p.desc ? (
                  <text x={x + 13} y={PATH_Y + 45} fontSize="9.5" className="fill-ink-ghost">
                    {p.desc}
                  </text>
                ) : null}
              </g>
            )
          })}

          {/* the single convergence target — accent border + accent spine */}
          <rect
            x={targetX}
            y={targetTop}
            width={TARGET_W}
            height={TARGET_H}
            className="fill-panel stroke-accent"
            strokeWidth={1.5}
          />
          <rect x={targetX} y={targetTop} width={3} height={TARGET_H} className="fill-accent" />
          <text
            x={targetCx}
            y={targetTop + (target.sub ? 36 : 49)}
            textAnchor="middle"
            fontSize="15"
            fontWeight={600}
            className="fill-ink"
          >
            {target.label}
          </text>
          {target.sub ? (
            <text
              x={targetCx}
              y={targetTop + 58}
              textAnchor="middle"
              fontSize="10"
              letterSpacing="0.03em"
              className="fill-ink-faint"
            >
              {target.sub}
            </text>
          ) : null}

          {/* the eliminated target box, off to one side */}
          {reject ? (
            <g>
              <rect
                x={rejectX}
                y={REJECT_Y}
                width={REJECT_W}
                height={REJECT_H}
                className="fill-bg stroke-alert"
                strokeWidth={1}
                strokeDasharray="5 4"
              />
              <text x={rejectX + 13} y={REJECT_Y + 24} fontSize="12" fontWeight={600} className="fill-alert">
                {reject.label}
              </text>
              {reject.desc ? (
                <text x={rejectX + 13} y={REJECT_Y + 43} fontSize="9.5" className="fill-ink-faint">
                  {reject.desc}
                </text>
              ) : null}
              <text
                x={rejectX + REJECT_W - 11}
                y={REJECT_Y + 24}
                textAnchor="end"
                fontSize="11"
                letterSpacing="0.06em"
                className="fill-alert uppercase"
              >
                {'✗ removed'}
              </text>
            </g>
          ) : null}
        </svg>
      </div>
    </div>
  )
}
