import { PlayerControls } from '@lib/components/PlayerControls'
import { FnChip } from '@lib/components/schematic/FnChip'
import { useStepper } from '@lib/hooks/useStepper'
import { cn } from '@lib/lib/utils'
import { useMemo } from 'react'

/**
 * A18 - a parent spawns N parallel children and joins their results: fan out,
 * work concurrently, join back, one answer. Steppable narrative states drive
 * which children exist, run, and have delivered. All content arrives via
 * props — child/state data lives in the deck's content/.
 */

export type SpawnChildState = 'hidden' | 'queued' | 'running' | 'done'
export type SpawnParentState = 'running' | 'parked' | 'resumed' | 'completed'

export interface SpawnTreeChild {
  id: string
  /** title line inside the child box, e.g. "sub-agent a — s_9c1" */
  label: string
  /** second line, e.g. "task: survey the provider landscape" */
  task: string
  /** third line, e.g. "policy: parent ∩ request · output: json + schema" */
  meta?: string
  /** x coordinate of the child box (viewBox is 980×320; boxes are 240×92) */
  x: number
}

export interface SpawnTreeState {
  title: string
  desc: string
  parent: SpawnParentState
  children: SpawnChildState[]
  /** per child: has its result edge been delivered back to the parent */
  joins: boolean[]
}

const CHILD_Y = 196
const CHILD_W = 240
const CHILD_H = 92
const PARENT = { x: 360, y: 36, w: 260, h: 78 }

export function SpawnTree({
  heading,
  chips = [],
  parentTitle,
  parentLabels,
  parentCallLine,
  nodes,
  states,
  ariaLabel,
  intervalMs = 3200,
  className,
}: {
  /** panel header, e.g. "harness::spawn — fan out, join back" */
  heading: string
  /** ghost chips in the header, e.g. ["depth ≤ 3", "fan-out ≤ 5"] */
  chips?: string[]
  /** title inside the parent box, e.g. "parent turn — s_7a1 / t_004" */
  parentTitle: string
  /** status line per parent state */
  parentLabels: Record<SpawnParentState, string>
  /** bottom line of the parent box, by step: e.g. first "model emits: spawn ×3", later "calls: 3 × spawn" */
  parentCallLine: { first: string; rest: string }
  nodes: SpawnTreeChild[]
  states: SpawnTreeState[]
  ariaLabel: string
  intervalMs?: number
  className?: string
}) {
  const stepper = useStepper(states.length, intervalMs)
  const state = states[stepper.step]
  const reducedMotion = useMemo(
    () => typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    [],
  )

  return (
    <div className={cn('border border-rule bg-bg', className)}>
      <div className="flex flex-wrap items-center justify-between gap-2 bg-panel px-3.5 py-2 border-b border-rule">
        <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">{heading}</span>
        {chips.length > 0 ? (
          <span className="flex gap-x-2">
            {chips.map((chip) => (
              <FnChip key={chip} tone="ghost">
                {chip}
              </FnChip>
            ))}
          </span>
        ) : null}
      </div>

      <div className="overflow-x-auto">
        <svg
          viewBox="0 0 980 320"
          className="w-full h-auto min-w-[760px] font-mono select-none"
          role="img"
          aria-label={ariaLabel}
        >
          <defs>
            <marker
              id="tree-arr"
              viewBox="0 0 8 8"
              refX="7"
              refY="4"
              markerWidth="6.5"
              markerHeight="6.5"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-ink-ghost" />
            </marker>
            <marker
              id="tree-arr-accent"
              viewBox="0 0 8 8"
              refX="7"
              refY="4"
              markerWidth="6.5"
              markerHeight="6.5"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-accent" />
            </marker>
          </defs>

          {/* parent node */}
          <rect
            x={PARENT.x}
            y={PARENT.y}
            width={PARENT.w}
            height={PARENT.h}
            strokeWidth={1.4}
            className={cn(
              'transition-all duration-300',
              state.parent === 'parked' ? 'fill-paper-2 stroke-warn' : 'fill-panel stroke-accent',
            )}
          />
          <text
            x={PARENT.x + PARENT.w / 2}
            y={PARENT.y + 26}
            textAnchor="middle"
            fontSize="13"
            fontWeight={600}
            className="fill-ink"
          >
            {parentTitle}
          </text>
          <text
            x={PARENT.x + PARENT.w / 2}
            y={PARENT.y + 45}
            textAnchor="middle"
            fontSize="9.5"
            letterSpacing="0.03em"
            className={cn(state.parent === 'parked' ? 'fill-warn' : 'fill-ink-faint')}
          >
            {parentLabels[state.parent]}
          </text>
          <text
            x={PARENT.x + PARENT.w / 2}
            y={PARENT.y + 62}
            textAnchor="middle"
            fontSize="9"
            className="fill-ink-ghost"
          >
            {stepper.step === 0 ? parentCallLine.first : parentCallLine.rest}
          </text>

          {/* spawn + join edges, per child */}
          {nodes.map((child, i) => {
            const cs = state.children[i]
            if (!cs || cs === 'hidden') return null
            const cx = child.x + CHILD_W / 2
            const px = PARENT.x + PARENT.w / 2 + (i - (nodes.length - 1) / 2) * 70
            const spawnD = `M ${px} ${PARENT.y + PARENT.h} C ${px} ${PARENT.y + PARENT.h + 40}, ${cx} ${CHILD_Y - 40}, ${cx} ${CHILD_Y}`
            const joinD = `M ${cx + 24} ${CHILD_Y} C ${cx + 24} ${CHILD_Y - 44}, ${px + 24} ${PARENT.y + PARENT.h + 36}, ${px + 24} ${PARENT.y + PARENT.h}`
            const joined = state.joins[i]
            return (
              <g key={child.id}>
                <path
                  d={spawnD}
                  fill="none"
                  strokeWidth={1}
                  className={cn('transition-[stroke]', cs === 'queued' ? 'stroke-rule' : 'stroke-ink-ghost')}
                  markerEnd="url(#tree-arr)"
                />
                {joined ? (
                  <>
                    <path
                      d={joinD}
                      fill="none"
                      strokeWidth={1.2}
                      strokeDasharray="5 4"
                      className="stroke-accent"
                      markerEnd="url(#tree-arr-accent)"
                    />
                    {!reducedMotion && stepper.step < states.length - 1 ? (
                      <circle r="2.4" className="fill-accent">
                        <animateMotion dur="1.6s" repeatCount="indefinite" path={joinD} />
                      </circle>
                    ) : null}
                  </>
                ) : null}
              </g>
            )
          })}

          {/* children */}
          {nodes.map((child, i) => {
            const cs = state.children[i]
            if (!cs || cs === 'hidden') return null
            return (
              <g key={child.id} className="transition-opacity duration-300">
                <rect
                  x={child.x}
                  y={CHILD_Y}
                  width={CHILD_W}
                  height={CHILD_H}
                  strokeWidth={cs === 'running' ? 1.4 : 1}
                  className={cn(
                    'transition-all duration-300',
                    cs === 'queued' && 'fill-bg stroke-rule',
                    cs === 'running' && 'fill-bg stroke-ink',
                    cs === 'done' && 'fill-paper-2 stroke-ink-faint',
                  )}
                />
                <circle
                  cx={child.x + 16}
                  cy={CHILD_Y + 19}
                  r={3}
                  className={cn(
                    cs === 'queued' && 'fill-ink-ghost',
                    cs === 'running' && 'fill-accent',
                    cs === 'done' && 'fill-ink-faint',
                  )}
                >
                  {cs === 'running' && !reducedMotion ? (
                    <animate attributeName="opacity" values="1;0.25;1" dur="1.2s" repeatCount="indefinite" />
                  ) : null}
                </circle>
                <text x={child.x + 28} y={CHILD_Y + 23} fontSize="11.5" fontWeight={600} className="fill-ink">
                  {child.label}
                </text>
                <text x={child.x + 16} y={CHILD_Y + 44} fontSize="9.5" className="fill-ink-faint">
                  {child.task}
                </text>
                {child.meta ? (
                  <text x={child.x + 16} y={CHILD_Y + 62} fontSize="9" className="fill-ink-ghost">
                    {child.meta}
                  </text>
                ) : null}
                <text
                  x={child.x + 16}
                  y={CHILD_Y + 78}
                  fontSize="9"
                  letterSpacing="0.05em"
                  className={cn('uppercase', cs === 'running' ? 'fill-accent' : 'fill-ink-ghost')}
                >
                  {cs === 'queued' ? 'queued' : cs === 'running' ? 'turn running' : 'completed ✓'}
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
          <span className="font-mono text-[14px] font-semibold lowercase text-ink">{state.title}</span>
        </div>
        <p className="mt-1.5 font-mono text-[12.5px] leading-[1.65] text-ink-faint lowercase max-w-[88ch]">
          {state.desc}
        </p>
      </div>

      <PlayerControls stepper={stepper} total={states.length} />
    </div>
  )
}
