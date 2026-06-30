import { useMemo } from 'react'
import { PlayerControls } from '@/components/PlayerControls'
import { FnChip } from '@/components/schematic/FnChip'
import { useStepper } from '@/hooks/useStepper'
import { cn } from '@/lib/utils'

export interface SeqLane {
  id: string
  label: string
  x: number
}

export interface SeqStep {
  from: string
  to: string // same as from => self loop
  label: string
  title: string
  desc: string
  event?: string // trigger type this step emits
}

interface SequencePlayerProps {
  title: string
  lanes: SeqLane[]
  steps: SeqStep[]
  width?: number
  intervalMs?: number
  className?: string
}

const ROW_H = 46
const TOP = 64

/**
 * a step-through sequence diagram: lifelines per lane, one arrow per step,
 * past steps in ink, the active step in accent with a travelling pulse.
 */
export function SequencePlayer({
  title,
  lanes,
  steps,
  width = 980,
  intervalMs = 2800,
  className,
}: SequencePlayerProps) {
  const stepper = useStepper(steps.length, intervalMs)
  const height = TOP + steps.length * ROW_H + 18
  const laneById = useMemo(
    () => Object.fromEntries(lanes.map((l) => [l.id, l])),
    [lanes],
  )
  const reducedMotion = useMemo(
    () =>
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    [],
  )
  const active = steps[stepper.step]
  const activeLanes = new Set([active.from, active.to])

  return (
    <div className={cn('border border-rule bg-bg', className)}>
      <div className="flex items-center justify-between bg-panel px-3.5 py-2 border-b border-rule">
        <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
          {title}
        </span>
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost tabular-nums">
          {steps.length} steps
        </span>
      </div>

      <div className="overflow-x-auto">
        <svg
          viewBox={`0 0 ${width} ${height}`}
          className="w-full h-auto min-w-[720px] font-mono select-none"
          role="img"
          aria-label={`sequence diagram: ${title}`}
        >
          <defs>
            <marker id="seq-arr-ink" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6.5" markerHeight="6.5" orient="auto-start-reverse">
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-ink-faint" />
            </marker>
            <marker id="seq-arr-accent" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6.5" markerHeight="6.5" orient="auto-start-reverse">
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-accent" />
            </marker>
          </defs>

          {/* lifelines + lane headers */}
          {lanes.map((lane) => {
            const hot = activeLanes.has(lane.id)
            return (
              <g key={lane.id}>
                <line
                  x1={lane.x}
                  y1={46}
                  x2={lane.x}
                  y2={height - 8}
                  strokeDasharray="2 5"
                  className={cn('transition-[stroke]', hot ? 'stroke-ink-faint' : 'stroke-rule')}
                  strokeWidth={1}
                />
                <rect
                  x={lane.x - 62}
                  y={14}
                  width={124}
                  height={26}
                  strokeWidth={hot ? 1.4 : 1}
                  className={cn(
                    'transition-all',
                    hot ? 'fill-panel stroke-accent' : 'fill-bg stroke-rule',
                  )}
                />
                <text
                  x={lane.x}
                  y={31}
                  textAnchor="middle"
                  fontSize="11"
                  fontWeight={600}
                  className={cn('transition-[fill]', hot ? 'fill-ink' : 'fill-ink-faint')}
                >
                  {lane.label}
                </text>
              </g>
            )
          })}

          {/* one row per revealed step */}
          {steps.map((step, i) => {
            if (i > stepper.step) return null
            const isActive = i === stepper.step
            const y = TOP + i * ROW_H
            const from = laneById[step.from]
            const to = laneById[step.to]
            const self = step.from === step.to
            const d = self
              ? `M ${from.x} ${y} C ${from.x + 64} ${y - 4}, ${from.x + 64} ${y + 22}, ${from.x + 6} ${y + 20}`
              : `M ${from.x} ${y} L ${to.x} ${y}`
            const midX = self ? from.x + 70 : (from.x + to.x) / 2
            return (
              <g key={`${step.label}-${i}`} className={cn(!isActive && 'opacity-75')}>
                <path
                  d={d}
                  fill="none"
                  strokeWidth={isActive ? 1.4 : 1}
                  markerEnd={`url(#${isActive ? 'seq-arr-accent' : 'seq-arr-ink'})`}
                  className={isActive ? 'stroke-accent' : 'stroke-ink-ghost'}
                />
                {isActive && !reducedMotion && !self ? (
                  <circle r="2.6" className="fill-accent">
                    <animateMotion dur="1.4s" repeatCount="indefinite" path={d} />
                  </circle>
                ) : null}
                <text
                  x={self ? from.x + 78 : midX}
                  y={y - 7}
                  textAnchor={self ? 'start' : 'middle'}
                  fontSize="10.5"
                  className={isActive ? 'fill-ink' : 'fill-ink-faint'}
                  style={{ paintOrder: 'stroke', stroke: 'var(--color-bg)', strokeWidth: 4 }}
                >
                  {step.label}
                </text>
                {step.event ? (
                  <g>
                    <circle
                      cx={self ? from.x + 70 : midX - (step.event.length * 5.4) / 2 - 10}
                      cy={y + 12}
                      r={2.4}
                      className={isActive ? 'fill-accent' : 'fill-ink-ghost'}
                    />
                    <text
                      x={self ? from.x + 78 : midX}
                      y={y + 16}
                      textAnchor={self ? 'start' : 'middle'}
                      fontSize="9"
                      letterSpacing="0.04em"
                      className="fill-ink-ghost"
                      style={{ paintOrder: 'stroke', stroke: 'var(--color-bg)', strokeWidth: 3.5 }}
                    >
                      fires {step.event}
                    </text>
                  </g>
                ) : null}
              </g>
            )
          })}
        </svg>
      </div>

      {/* active step narration */}
      <div className="border-t border-rule px-4 py-3.5 min-h-[92px]">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
          <span className="font-mono text-[11px] text-ink-ghost tabular-nums">
            {String(stepper.step + 1).padStart(2, '0')}
          </span>
          <span className="font-mono text-[14px] font-semibold lowercase text-ink">
            {active.title}
          </span>
          <FnChip tone="accent">{active.label}</FnChip>
        </div>
        <p className="mt-1.5 font-mono text-[12.5px] leading-[1.65] text-ink-faint lowercase max-w-[88ch]">
          {active.desc}
        </p>
      </div>

      <PlayerControls stepper={stepper} total={steps.length} />
    </div>
  )
}
