import { useMemo } from 'react'
import { PlayerControls } from '@/components/PlayerControls'
import { FnChip } from '@/components/schematic/FnChip'
import { useStepper } from '@/hooks/useStepper'
import { cn } from '@/lib/utils'

export interface RevealStage {
  label: string // short stage name, e.g. "crash"
  tone?: 'ink' | 'accent' | 'alert' | 'warn' // accent for this stage box
  caption?: string // one-line description shown when active
  rows?: { k: string; v: string }[] // a small record that evolves per stage
  note?: string // optional extra faint line for the active stage
}

export interface StepRevealProps {
  title: string
  stages: RevealStage[]
  intervalMs?: number
  className?: string
}

type StageTone = NonNullable<RevealStage['tone']>

const toneClasses: Record<StageTone, { box: string; active: string; label: string }> = {
  ink: { box: 'border-rule', active: 'border-accent bg-panel', label: 'text-ink' },
  accent: { box: 'border-rule', active: 'border-accent bg-panel', label: 'text-accent' },
  alert: { box: 'border-alert/50', active: 'border-alert bg-panel', label: 'text-alert' },
  warn: { box: 'border-warn/50', active: 'border-warn bg-panel', label: 'text-warn' },
}

// a glyph hint per tone, mirroring the reference timeline's stage markers
const toneGlyph: Record<StageTone, string> = {
  ink: '',
  accent: '',
  alert: '⚡ ',
  warn: '⏸ ',
}

/**
 * archetype A6 — a step-through LIFECYCLE / TIMELINE walker.
 *
 * use when the spec describes a process whose STATE evolves across ordered
 * stages: a boot sequence, a durability timeline, a readiness bring-up, a
 * crash-and-resume story. a horizontal strip of numbered stage boxes is
 * connected left-to-right; the stepper walks them — past stages dim, the
 * active stage lights up per its tone — while a panel below shows that
 * stage's caption, its evolving k/v record, and an optional note.
 */
export function StepReveal({
  title,
  stages,
  intervalMs = 2400,
  className,
}: StepRevealProps) {
  const stepper = useStepper(stages.length, intervalMs)
  const reducedMotion = useMemo(
    () =>
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    [],
  )
  const active = stages[stepper.step]
  const activeTone: StageTone = active.tone ?? 'ink'

  // narrow strips scroll-x; give each box room to breathe past ~7 stages
  const minWidth = Math.max(640, stages.length * 132)

  return (
    <div className={cn('border border-rule bg-bg', className)}>
      <div className="flex items-center justify-between bg-panel px-3.5 py-2 border-b border-rule">
        <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
          {title}
        </span>
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost tabular-nums">
          {stages.length} stages
        </span>
      </div>

      {/* the strip — numbered stage boxes connected left-to-right */}
      <div className="overflow-x-auto">
        <div
          className="flex items-stretch gap-0 px-4 py-6"
          style={{ minWidth: `${minWidth}px` }}
        >
          {stages.map((stage, i) => {
            const reached = i <= stepper.step
            const isActive = i === stepper.step
            const tone = toneClasses[stage.tone ?? 'ink']
            return (
              <div key={`${stage.label}-${i}`} className="flex items-center flex-1 min-w-0">
                <button
                  type="button"
                  onClick={() => stepper.goTo(i)}
                  aria-current={isActive ? 'step' : undefined}
                  className={cn(
                    'border bg-bg px-2.5 py-2 w-full min-w-0 text-left transition-all duration-200 cursor-pointer',
                    reached ? tone.box : 'border-rule-2 opacity-45',
                    isActive && tone.active,
                    isActive && !reducedMotion && 'fade-rise',
                  )}
                >
                  <div className="flex items-baseline gap-1.5">
                    <span className="font-mono text-[9px] tabular-nums shrink-0 text-ink-ghost">
                      {String(i + 1).padStart(2, '0')}
                    </span>
                    <span
                      className={cn(
                        'font-mono text-[10.5px] font-semibold leading-[1.35] truncate',
                        reached ? tone.label : 'text-ink-ghost',
                      )}
                    >
                      {toneGlyph[stage.tone ?? 'ink']}
                      {stage.label}
                    </span>
                  </div>
                </button>
                {i < stages.length - 1 ? (
                  <span
                    aria-hidden
                    className={cn(
                      'h-px w-4 shrink-0',
                      i < stepper.step ? 'bg-ink-faint' : 'bg-rule',
                    )}
                  />
                ) : null}
              </div>
            )
          })}
        </div>
      </div>

      {/* active stage panel — caption + the evolving record + note */}
      <div className="border-t border-rule px-4 py-3.5 min-h-[120px]">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
          <span className="font-mono text-[11px] text-ink-ghost tabular-nums">
            {String(stepper.step + 1).padStart(2, '0')}
          </span>
          <span
            className={cn(
              'font-mono text-[14px] font-semibold lowercase',
              activeTone === 'alert'
                ? 'text-alert'
                : activeTone === 'warn'
                  ? 'text-warn'
                  : activeTone === 'accent'
                    ? 'text-accent'
                    : 'text-ink',
            )}
          >
            {active.label}
          </span>
        </div>

        {active.caption ? (
          <p className="mt-1.5 font-mono text-[12.5px] leading-[1.65] text-ink-faint lowercase max-w-[88ch]">
            {active.caption}
          </p>
        ) : null}

        {active.rows && active.rows.length > 0 ? (
          <dl className="mt-2.5 font-mono text-[11.5px] leading-[1.8]">
            {active.rows.map((row) => (
              <div key={row.k} className="flex gap-x-2">
                <dt className="text-ink-ghost w-20 shrink-0 truncate">{row.k}</dt>
                <dd className="text-ink tabular-nums">{row.v}</dd>
              </div>
            ))}
          </dl>
        ) : null}

        {active.note ? (
          <div className="mt-2.5">
            <FnChip tone="faint" className="whitespace-normal">
              {active.note}
            </FnChip>
          </div>
        ) : null}
      </div>

      <PlayerControls stepper={stepper} total={stages.length} />
    </div>
  )
}
