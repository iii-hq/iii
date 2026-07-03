import { useMemo } from 'react'
import { PlayerControls } from '@/components/PlayerControls'
import { FnChip } from '@/components/schematic/FnChip'
import { SEQ_LANES, SEQ_STEPS } from '@/content/changes'
import { useStepper } from '@/hooks/useStepper'
import { cn } from '@/lib/utils'

const ROW_H = 52
const TOP = 70
const WIDTH = 1080

export function MeetingPointsSequence({ className }: { className?: string }) {
  const stepper = useStepper(SEQ_STEPS.length, 3000)
  const height = TOP + SEQ_STEPS.length * ROW_H + 20
  const laneById = useMemo(
    () => Object.fromEntries(SEQ_LANES.map((l) => [l.id, l])),
    [],
  )
  const reducedMotion = useMemo(
    () =>
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    [],
  )
  const active = SEQ_STEPS[stepper.step]
  const activeLanes = new Set([active.from, active.to])

  return (
    <div className={cn('border border-rule bg-bg', className)}>
      <div className="flex items-center justify-between bg-panel px-3.5 py-2 border-b border-rule">
        <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
          one node coming up during `iii up`
        </span>
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost tabular-nums">
          {SEQ_STEPS.length} steps
        </span>
      </div>

      <div className="overflow-x-auto">
        <svg
          viewBox={`0 0 ${WIDTH} ${height}`}
          className="w-full h-auto min-w-[760px] font-mono select-none"
          role="img"
          aria-label="sequence: the three meeting points during an up"
        >
          <defs>
            <marker id="seq-ink" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6.5" markerHeight="6.5" orient="auto-start-reverse">
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-ink-faint" />
            </marker>
            <marker id="seq-accent" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6.5" markerHeight="6.5" orient="auto-start-reverse">
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-accent" />
            </marker>
          </defs>

          {SEQ_LANES.map((lane) => {
            const hot = activeLanes.has(lane.id)
            return (
              <g key={lane.id}>
                <line
                  x1={lane.x}
                  y1={50}
                  x2={lane.x}
                  y2={height - 8}
                  strokeDasharray="2 5"
                  className={cn('transition-[stroke]', hot ? 'stroke-ink-faint' : 'stroke-rule')}
                  strokeWidth={1}
                />
                <rect
                  x={lane.x - 70}
                  y={14}
                  width={140}
                  height={28}
                  strokeWidth={hot ? 1.4 : 1}
                  className={cn('transition-all', hot ? 'fill-panel stroke-accent' : 'fill-bg stroke-rule')}
                />
                <text
                  x={lane.x}
                  y={32}
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

          {SEQ_STEPS.map((step, i) => {
            if (i > stepper.step) return null
            const isActive = i === stepper.step
            const y = TOP + i * ROW_H
            const from = laneById[step.from]
            const to = laneById[step.to]
            const self = step.from === step.to
            const d = self
              ? `M ${from.x} ${y} C ${from.x + 70} ${y - 6}, ${from.x + 70} ${y + 26}, ${from.x + 6} ${y + 22}`
              : `M ${from.x} ${y} L ${to.x} ${y}`
            const midX = self ? from.x + 80 : (from.x + to.x) / 2
            return (
              <g key={i} className={cn(!isActive && 'opacity-70')}>
                <path
                  d={d}
                  fill="none"
                  strokeWidth={isActive ? 1.5 : 1}
                  strokeDasharray={step.dashed ? '5 4' : undefined}
                  markerEnd={`url(#${isActive ? 'seq-accent' : 'seq-ink'})`}
                  className={isActive ? 'stroke-accent' : 'stroke-ink-ghost'}
                />
                {isActive && !reducedMotion && !self ? (
                  <circle r="2.8" className="fill-accent">
                    <animateMotion dur="1.5s" repeatCount="indefinite" path={d} />
                  </circle>
                ) : null}
                {step.meeting ? (
                  <>
                    <circle
                      cx={midX - (step.label.length * 5.4) / 2 - 14}
                      cy={y - 7}
                      r="7.5"
                      className={cn(isActive ? 'fill-accent stroke-accent' : 'fill-bg stroke-ink-faint')}
                      strokeWidth={1}
                    />
                    <text
                      x={midX - (step.label.length * 5.4) / 2 - 14}
                      y={y - 3.5}
                      textAnchor="middle"
                      fontSize="9"
                      className={isActive ? 'fill-bg' : 'fill-ink-faint'}
                    >
                      {step.meeting}
                    </text>
                  </>
                ) : null}
                <text
                  x={self ? from.x + 92 : midX}
                  y={y - 7}
                  textAnchor={self ? 'start' : 'middle'}
                  fontSize="10.5"
                  className={isActive ? 'fill-ink' : 'fill-ink-faint'}
                  style={{ paintOrder: 'stroke', stroke: 'var(--color-bg)', strokeWidth: 4 }}
                >
                  {step.label}
                </text>
              </g>
            )
          })}
        </svg>
      </div>

      <div className="border-t border-rule px-4 py-3.5 min-h-[96px]">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
          <span className="font-mono text-[11px] text-ink-ghost tabular-nums">
            {String(stepper.step + 1).padStart(2, '0')}
          </span>
          <span className="font-mono text-[14px] font-semibold lowercase text-ink">
            {active.title}
          </span>
          {active.meeting ? <FnChip tone="accent">meeting {active.meeting}</FnChip> : null}
        </div>
        <p className="mt-1.5 font-mono text-[12.5px] leading-[1.65] text-ink-faint lowercase max-w-[92ch]">
          {active.desc}
        </p>
      </div>

      <PlayerControls stepper={stepper} total={SEQ_STEPS.length} />
    </div>
  )
}
