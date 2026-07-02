import { PlayerControls } from '@lib/components/PlayerControls'
import { FnChip } from '@lib/components/schematic/FnChip'
import { useStepper } from '@lib/hooks/useStepper'
import { cn } from '@lib/lib/utils'

/**
 * A16 - a lifecycle told as a steppable timeline of stages, each stage pairing
 * a narration with an evolving state record beside it. Built for "a durable
 * thing survives crashes and waits" stories; generic over any staged record.
 * All content arrives via props — stage data lives in the deck's content/.
 */

export type TimelineTone = 'ink' | 'alert' | 'warn' | 'accent'

export interface TimelineStage {
  id: string
  label: string
  sub: string
  tone: TimelineTone
  title: string
  desc: string
  record: { status: string; step: string; calls: string; note: string }
  /** render the connector after this stage dashed (a pause / long wait) */
  gapAfter?: boolean
}

const toneClasses: Record<TimelineTone, { box: string; active: string; label: string }> = {
  ink: { box: 'border-rule', active: 'border-ink bg-panel', label: 'text-ink' },
  alert: { box: 'border-alert/50', active: 'border-alert bg-panel', label: 'text-alert' },
  warn: { box: 'border-warn/50', active: 'border-warn bg-panel', label: 'text-warn' },
  accent: { box: 'border-rule', active: 'border-accent bg-panel', label: 'text-accent' },
}

export function DurabilityTimeline({
  stages,
  heading,
  headingNote,
  recordHeading,
  intervalMs = 3000,
  className,
}: {
  stages: TimelineStage[]
  /** panel header, e.g. "one turn vs. a crash and a long wait" */
  heading: string
  /** right-hand header note, e.g. "turn t_001" */
  headingNote?: string
  /** caption above the record panel, e.g. "the turn record — state, not a process" */
  recordHeading: string
  intervalMs?: number
  className?: string
}) {
  const stepper = useStepper(stages.length, intervalMs)
  const active = stages[stepper.step]

  return (
    <div className={cn('border border-rule bg-bg', className)}>
      <div className="flex items-center justify-between bg-panel px-3.5 py-2 border-b border-rule">
        <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">{heading}</span>
        {headingNote ? (
          <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost tabular-nums">
            {headingNote}
          </span>
        ) : null}
      </div>

      {/* the track */}
      <div className="overflow-x-auto">
        <div className="flex items-stretch gap-0 px-4 py-6 min-w-[860px]">
          {stages.map((stage, i) => {
            const reached = i <= stepper.step
            const isActive = i === stepper.step
            const tone = toneClasses[stage.tone]
            return (
              <div key={stage.id} className="flex items-center flex-1 min-w-0">
                <button
                  type="button"
                  onClick={() => stepper.goTo(i)}
                  className={cn(
                    'border bg-bg px-2.5 py-2 w-full min-w-0 text-left transition-all duration-200 cursor-pointer',
                    reached ? tone.box : 'border-rule-2 opacity-45',
                    isActive && tone.active,
                    isActive && stage.tone === 'ink' && 'border-accent',
                  )}
                >
                  <div
                    className={cn(
                      'font-mono text-[10.5px] font-semibold leading-[1.35] truncate',
                      reached ? tone.label : 'text-ink-ghost',
                      isActive && stage.tone === 'ink' && 'text-ink',
                    )}
                  >
                    {stage.tone === 'alert' ? '⚡ ' : stage.tone === 'warn' ? '⏸ ' : ''}
                    {stage.label}
                  </div>
                  <div className="mt-0.5 font-mono text-[9px] tracking-[0.03em] text-ink-ghost truncate">
                    {stage.sub}
                  </div>
                </button>
                {i < stages.length - 1 ? (
                  <span
                    aria-hidden
                    className={cn(
                      'h-px w-4 shrink-0',
                      i < stepper.step ? 'bg-ink-faint' : 'bg-rule',
                      stage.gapAfter && 'w-6 border-t border-dashed border-rule bg-transparent',
                    )}
                  />
                ) : null}
              </div>
            )
          })}
        </div>
      </div>

      {/* the evolving record beside the narration */}
      <div className="grid grid-cols-1 @3xl:grid-cols-[minmax(0,7fr)_minmax(0,5fr)] border-t border-rule">
        <div className="px-4 py-3.5 min-h-[108px]">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
            <span className="font-mono text-[11px] text-ink-ghost tabular-nums">
              {String(stepper.step + 1).padStart(2, '0')}
            </span>
            <span className="font-mono text-[14px] font-semibold lowercase text-ink">{active.title}</span>
          </div>
          <p className="mt-1.5 font-mono text-[12.5px] leading-[1.65] text-ink-faint lowercase max-w-[78ch]">
            {active.desc}
          </p>
        </div>
        <div className="border-t @3xl:border-t-0 @3xl:border-l border-rule-2 bg-paper-2 px-4 py-3.5">
          <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint mb-2">{recordHeading}</div>
          <dl className="font-mono text-[11.5px] leading-[1.8]">
            <div className="flex gap-x-2">
              <dt className="text-ink-ghost w-14 shrink-0">status</dt>
              <dd className="text-ink">{active.record.status}</dd>
            </div>
            <div className="flex gap-x-2">
              <dt className="text-ink-ghost w-14 shrink-0">step</dt>
              <dd className="text-ink tabular-nums">{active.record.step}</dd>
            </div>
            <div className="flex gap-x-2">
              <dt className="text-ink-ghost w-14 shrink-0">calls</dt>
              <dd className="text-ink">{active.record.calls}</dd>
            </div>
          </dl>
          <div className="mt-2">
            <FnChip tone="faint" className="whitespace-normal">
              {active.record.note}
            </FnChip>
          </div>
        </div>
      </div>

      <PlayerControls stepper={stepper} total={stages.length} />
    </div>
  )
}
