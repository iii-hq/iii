import { useEffect, useMemo, useState } from 'react'
import { Button } from '@/components/schematic/Button'
import { Caret } from '@/components/schematic/Caret'
import { ModeToggle } from '@/components/schematic/ModeToggle'
import { Prompt } from '@/components/schematic/Prompt'
import { useStepper } from '@/hooks/useStepper'
import { cn } from '@/lib/utils'
import type { PlayCommand, PlayTrack } from '@/content/playground'

/** tint one output line by its leading glyph — accent rationed to ✓ / results */
function lineClass(line: string): string {
  const t = line.trimStart()
  if (t.startsWith('✓')) return 'text-accent'
  if (t.startsWith('✗')) return 'text-alert'
  if (t.startsWith('!')) return 'text-warn'
  if (t.startsWith('{') || t.startsWith('"')) return 'text-ink'
  if (t.startsWith('ID ') || t.startsWith('ID\t')) return 'text-ink-ghost uppercase tracking-[0.06em]'
  if (t.startsWith('→') || t.startsWith('  ') || t.startsWith('compose::') || t.startsWith('all ') || t.startsWith('attached'))
    return 'text-ink-faint'
  return 'text-ink-faint'
}

function CommandBlock({
  step,
  active,
  trackId,
}: {
  step: PlayCommand
  active: boolean
  trackId: string
}) {
  const [showFn, setShowFn] = useState(false)
  return (
    <div className="pb-3">
      <div className="flex items-start gap-x-2">
        <span className="font-mono text-[13px] text-accent shrink-0 select-none">$</span>
        <span className="font-mono text-[13px] text-ink break-all">{step.cmd}</span>
        {active ? <Caret /> : null}
      </div>

      <button
        type="button"
        onClick={() => setShowFn((v) => !v)}
        className="mt-1 ml-4 flex items-start gap-x-1.5 text-left font-mono text-[11px] text-ink-ghost hover:text-ink-faint transition-colors cursor-pointer"
      >
        <span className="text-accent shrink-0">ƒ</span>
        <span className={cn('leading-[1.5]', !showFn && 'truncate max-w-[60ch]')}>
          {step.fn}
        </span>
      </button>

      {/* output streams in, one line per row, staggered for a typed feel */}
      <div key={`${trackId}-${step.id}`} className="mt-2 ml-4 flex flex-col gap-y-0.5">
        {step.output.map((line, i) => (
          <pre
            key={i}
            className={cn(
              'font-mono text-[12.5px] leading-[1.5] whitespace-pre-wrap break-words',
              lineClass(line),
              active && 'fade-rise',
            )}
            style={active ? { animationDelay: `${Math.min(i * 70, 700)}ms` } : undefined}
          >
            {line || ' '}
          </pre>
        ))}
        {typeof step.exit === 'number' ? (
          <pre className="mt-1 font-mono text-[11px] uppercase tracking-[0.08em] text-alert">
            exit {step.exit}
          </pre>
        ) : null}
      </div>
    </div>
  )
}

interface CliPlaygroundProps {
  tracks: PlayTrack[]
  className?: string
  autoPlay?: boolean
}

export function CliPlayground({ tracks, className, autoPlay }: CliPlaygroundProps) {
  const [trackId, setTrackId] = useState(tracks[0].id)
  const track = useMemo(
    () => tracks.find((t) => t.id === trackId) ?? tracks[0],
    [tracks, trackId],
  )
  const stepper = useStepper(track.steps.length, 2600, autoPlay)

  // reset the transcript when the track changes
  useEffect(() => {
    stepper.reset()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [trackId])

  const revealed = track.steps.slice(0, stepper.step + 1)

  return (
    <div className={cn('border border-rule bg-bg', className)}>
      {/* chrome */}
      <div className="flex flex-wrap items-center justify-between gap-2 bg-panel px-3.5 py-2 border-b border-rule">
        <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
          iii — simulated terminal
        </span>
        <ModeToggle
          value={trackId}
          onChange={setTrackId}
          options={tracks.map((t) => ({ value: t.id, label: t.label }))}
        />
      </div>

      <div className="px-3.5 py-2 border-b border-rule-2 font-mono text-[11px] text-ink-ghost lowercase">
        {track.blurb}
      </div>

      {/* transcript */}
      <div className="px-4 py-4 min-h-[280px] max-h-[460px] overflow-y-auto [scrollbar-gutter:stable]">
        {revealed.map((step, i) => (
          <CommandBlock
            key={`${track.id}-${step.id}`}
            step={step}
            active={i === stepper.step}
            trackId={track.id}
          />
        ))}
        {stepper.atEnd ? (
          <div className="flex items-center gap-x-2 pt-1">
            <Prompt symbol="$" />
            <Caret />
          </div>
        ) : null}
      </div>

      {/* controls */}
      <div className="flex flex-wrap items-center gap-x-2 gap-y-2 border-t border-rule bg-panel px-3.5 py-2.5">
        <Button variant="pill" size="sm" onClick={stepper.toggle}>
          {stepper.playing ? 'pause' : stepper.atEnd ? 'replay' : 'play all'}
        </Button>
        <Button variant="icon" size="icon" aria-label="previous command" onClick={stepper.prev}>
          ←
        </Button>
        <Button variant="ghost" size="sm" onClick={stepper.next} disabled={stepper.atEnd}>
          run next →
        </Button>
        <Button variant="ghost" size="sm" onClick={stepper.reset}>
          reset
        </Button>
        <span className="ml-auto font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint tabular-nums">
          {track.label} — step {stepper.step + 1}/{track.steps.length}
        </span>
      </div>
    </div>
  )
}
