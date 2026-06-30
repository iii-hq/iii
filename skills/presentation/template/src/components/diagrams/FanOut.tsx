import { useMemo } from 'react'
import { cn } from '@/lib/utils'

/**
 * archetype A7 — one event fans out to many handlers.
 * use when the spec has a reactive/event model: one write/emit triggers
 * N bound handlers. ambient (always-on), no stepper: a source box emits a
 * trigger that ripples out and fans to every bound handler at once.
 */

export interface FanHandler {
  id: string
  label: string
  desc?: string
}

interface FanOutProps {
  source: { label: string; sub?: string } // the write/source box on the left
  trigger: string // the event/trigger type emitted (keeps its casing)
  handlers: FanHandler[] // 2-5 handlers it fans to, stacked on the right
  title?: string
  className?: string
}

// fixed horizontal geometry; vertical positions are derived from handler count
const SOURCE_X = 24
const SOURCE_W = 230
const SOURCE_H = 64
const TRIGGER_X = 368
const TRIGGER_W = 212
const TRIGGER_H = 52
const HANDLER_X = 732
const HANDLER_W = 228
const HANDLER_H = 46
const ROW_GAP = 26 // vertical gap between handler boxes
const TOP = 30 // top padding above the first handler

export function FanOut({
  source,
  trigger,
  handlers,
  title = 'one write, every handler',
  className,
}: FanOutProps) {
  const reducedMotion = useMemo(
    () =>
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    [],
  )

  // total drawing height grows with the handler count; the source + trigger
  // nodes center on the vertical midpoint of the stacked handlers.
  const stackH = handlers.length * HANDLER_H + (handlers.length - 1) * ROW_GAP
  const height = TOP + stackH + TOP
  const midY = TOP + stackH / 2

  // emit point: the right edge of the trigger node, on the centerline
  const emitX = TRIGGER_X + TRIGGER_W
  const emitY = midY

  return (
    <div className={cn('border border-rule bg-bg', className)}>
      <div className="flex items-center justify-between bg-panel px-3.5 py-2 border-b border-rule">
        <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
          {title}
        </span>
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost tabular-nums">
          fans to {handlers.length}
        </span>
      </div>

      <div className="overflow-x-auto">
        <svg
          viewBox={`0 0 980 ${height}`}
          className="w-full h-auto min-w-[720px] font-mono select-none"
          role="group"
          aria-label={`${source.label} emits ${trigger}, fanning out to ${handlers.length} bound handlers`}
        >
          <defs>
            <marker id="fan-arr" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6.5" markerHeight="6.5" orient="auto-start-reverse">
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-accent" />
            </marker>
            <marker id="fan-arr-ink" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6.5" markerHeight="6.5" orient="auto-start-reverse">
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-ink-ghost" />
            </marker>
          </defs>

          {/* the write / source */}
          <rect
            x={SOURCE_X}
            y={midY - SOURCE_H / 2}
            width={SOURCE_W}
            height={SOURCE_H}
            strokeWidth={1.25}
            className="fill-bg stroke-ink"
          />
          <text
            x={SOURCE_X + SOURCE_W / 2}
            y={source.sub ? midY - 5 : midY + 4}
            textAnchor="middle"
            fontSize="12.5"
            fontWeight={600}
            className="fill-ink"
          >
            {source.label}
          </text>
          {source.sub ? (
            <text
              x={SOURCE_X + SOURCE_W / 2}
              y={midY + 14}
              textAnchor="middle"
              fontSize="9.5"
              letterSpacing="0.05em"
              className="fill-ink-ghost"
            >
              {source.sub}
            </text>
          ) : null}

          {/* source → trigger node */}
          <path
            d={`M ${SOURCE_X + SOURCE_W} ${midY} L ${TRIGGER_X - 4} ${midY}`}
            fill="none"
            strokeWidth={1}
            className="stroke-ink-ghost"
            markerEnd="url(#fan-arr-ink)"
          />
          <text
            x={(SOURCE_X + SOURCE_W + TRIGGER_X) / 2}
            y={midY - 10}
            textAnchor="middle"
            fontSize="9"
            letterSpacing="0.04em"
            className="fill-ink-ghost"
            style={{ paintOrder: 'stroke', stroke: 'var(--color-bg)', strokeWidth: 3.5 }}
          >
            emits
          </text>

          {/* the trigger type node */}
          <rect
            x={TRIGGER_X}
            y={midY - TRIGGER_H / 2}
            width={TRIGGER_W}
            height={TRIGGER_H}
            strokeWidth={1.4}
            className="fill-panel stroke-accent"
          />
          <text
            x={TRIGGER_X + TRIGGER_W / 2}
            y={midY - 3}
            textAnchor="middle"
            fontSize="12"
            fontWeight={600}
            className="fill-ink"
          >
            {trigger}
          </text>
          <text
            x={TRIGGER_X + TRIGGER_W / 2}
            y={midY + 14}
            textAnchor="middle"
            fontSize="9"
            letterSpacing="0.06em"
            className="fill-ink-faint uppercase"
          >
            trigger type
          </text>

          {/* ambient ripple emanating from the emit point */}
          {!reducedMotion ? (
            <>
              <circle cx={emitX} cy={emitY} className="fill-none stroke-accent ripple-ring" strokeWidth={1} />
              <circle cx={emitX} cy={emitY} className="fill-none stroke-accent ripple-ring" strokeWidth={1} style={{ animationDelay: '0.8s' }} />
            </>
          ) : null}

          {/* fan-out edges + bound handlers */}
          {handlers.map((handler, i) => {
            const hy = TOP + i * (HANDLER_H + ROW_GAP) // top of this handler box
            const targetY = hy + HANDLER_H / 2 // centerline of this handler
            // smooth cubic from the emit point to the handler's left edge
            const cx1 = emitX + 56
            const cx2 = HANDLER_X - 56
            const d = `M ${emitX} ${emitY} C ${cx1} ${emitY}, ${cx2} ${targetY}, ${HANDLER_X} ${targetY}`
            // stagger pulse timing + travel duration so edges don't beat in sync
            const dur = 1.5 + (i % 3) * 0.3
            return (
              <g key={handler.id}>
                <path
                  d={d}
                  fill="none"
                  strokeWidth={1.1}
                  className="stroke-accent flow-dash"
                  markerEnd="url(#fan-arr)"
                />
                {!reducedMotion ? (
                  <circle r="2.6" className="fill-accent">
                    <animateMotion dur={`${dur}s`} begin={`${i * 0.25}s`} repeatCount="indefinite" path={d} />
                  </circle>
                ) : null}

                <rect
                  x={HANDLER_X}
                  y={hy}
                  width={HANDLER_W}
                  height={HANDLER_H}
                  strokeWidth={1}
                  className="fill-bg stroke-rule"
                />
                <text
                  x={HANDLER_X + HANDLER_W / 2}
                  y={handler.desc ? hy + 20 : hy + HANDLER_H / 2 + 4}
                  textAnchor="middle"
                  fontSize="11.5"
                  fontWeight={600}
                  className="fill-ink"
                >
                  {handler.label}
                </text>
                {handler.desc ? (
                  <text
                    x={HANDLER_X + HANDLER_W / 2}
                    y={hy + 36}
                    textAnchor="middle"
                    fontSize="9"
                    letterSpacing="0.04em"
                    className="fill-ink-faint"
                  >
                    {handler.desc}
                  </text>
                ) : null}
              </g>
            )
          })}

          {/* caption under the handler stack */}
          <text
            x={HANDLER_X + HANDLER_W / 2}
            y={height - 10}
            textAnchor="middle"
            fontSize="9"
            letterSpacing="0.06em"
            className="fill-ink-ghost uppercase"
          >
            bound handlers — subscribe once, run on every emit
          </text>
        </svg>
      </div>
    </div>
  )
}
