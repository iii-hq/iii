import { PlayerControls } from '@lib/components/PlayerControls'
import { FnChip } from '@lib/components/schematic/FnChip'
import { ModeToggle } from '@lib/components/schematic/ModeToggle'
import { Prompt } from '@lib/components/schematic/Prompt'
import { useStepper } from '@lib/hooks/useStepper'
import { cn } from '@lib/lib/utils'
import { useEffect, useMemo, useState } from 'react'

/**
 * archetype A3 — interactive terminal step-player.
 *
 * use for a command sequence: a golden-path install, a day-2 ops demo, the
 * "show it, don't tell it" proof slide where the transcript IS the argument.
 * the stepper reveals one line at a time; output lines stream in with a typed
 * feel, tinted by leading glyph (✓ accent, ✗ alert, ! warn, else faint).
 * pass >1 track to get a ModeToggle that switches between transcripts.
 */

export interface CliLine {
  cmd?: string // a command, rendered after a $ prompt
  out?: string[] // output lines; tinted by leading glyph (see lineClass)
  fn?: string // optional backing function id, shown as a faint FnChip
  exit?: number // optional exit code, shown subtly under the output
}

export interface CliTrack {
  id: string
  label: string
  lines: CliLine[]
}

interface CliPlaygroundProps {
  tracks: CliTrack[]
  title?: string
  intervalMs?: number
  className?: string
}

/** tint one output line by its leading glyph — accent stays rationed to ✓ / results */
function lineClass(line: string): string {
  const t = line.trimStart()
  if (t.startsWith('✓')) return 'text-accent'
  if (t.startsWith('✗')) return 'text-alert'
  if (t.startsWith('!')) return 'text-warn'
  if (t.startsWith('{') || t.startsWith('"')) return 'text-ink'
  if (t.startsWith('→')) return 'text-ink-faint'
  return 'text-ink-faint'
}

function CommandBlock({ line, active, blinkKey }: { line: CliLine; active: boolean; blinkKey: string }) {
  return (
    <div className="pb-3">
      {line.cmd ? (
        <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
          <Prompt symbol="$" className="text-[13px] shrink-0 select-none" />
          <span className="font-mono text-[13px] text-ink break-all">{line.cmd}</span>
          {line.fn ? (
            <FnChip tone="faint" className="ml-1">
              {line.fn}
            </FnChip>
          ) : null}
        </div>
      ) : null}

      {/* output streams in, one line per row, staggered for a typed feel */}
      {line.out && line.out.length ? (
        <div key={blinkKey} className={cn('flex flex-col gap-y-0.5', line.cmd && 'mt-2 ml-4')}>
          {line.out.map((text, i) => (
            <pre
              key={i}
              className={cn(
                'font-mono text-[12.5px] leading-[1.5] whitespace-pre-wrap break-words',
                lineClass(text),
                active && 'fade-rise',
              )}
              style={active ? { animationDelay: `${Math.min(i * 70, 700)}ms` } : undefined}
            >
              {text || ' '}
            </pre>
          ))}
        </div>
      ) : null}

      {typeof line.exit === 'number' ? (
        <pre
          className={cn(
            'mt-1 font-mono text-[11px] uppercase tracking-[0.08em] tabular-nums',
            line.cmd && 'ml-4',
            line.exit === 0 ? 'text-ink-ghost' : 'text-alert',
          )}
        >
          exit {line.exit}
        </pre>
      ) : null}
    </div>
  )
}

export function CliPlayground({
  tracks,
  title = 'simulated terminal',
  intervalMs = 2600,
  className,
}: CliPlaygroundProps) {
  const [trackId, setTrackId] = useState(tracks[0].id)
  const track = useMemo(() => tracks.find((t) => t.id === trackId) ?? tracks[0], [tracks, trackId])
  const stepper = useStepper(track.lines.length, intervalMs)

  // reset the transcript whenever the active track changes
  useEffect(() => {
    stepper.goTo(0)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [trackId])

  const revealed = track.lines.slice(0, stepper.step + 1)
  const multi = tracks.length > 1

  return (
    <div className={cn('border border-rule bg-bg', className)}>
      {/* panel header: title + a track toggle when there is more than one */}
      <div className="flex flex-wrap items-center justify-between gap-2 bg-panel px-3.5 py-2 border-b border-rule">
        <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">{title}</span>
        {multi ? (
          <ModeToggle
            value={trackId}
            onChange={setTrackId}
            options={tracks.map((t) => ({ value: t.id, label: t.label }))}
          />
        ) : null}
      </div>

      {/* transcript — height bounded, scrolls when the track runs long */}
      <div className="px-4 py-4 min-h-[280px] max-h-[460px] overflow-y-auto [scrollbar-gutter:stable]">
        {revealed.map((line, i) => (
          <CommandBlock
            key={`${track.id}-${i}`}
            line={line}
            active={i === stepper.step}
            blinkKey={`${track.id}-${i}`}
          />
        ))}
        {stepper.atEnd ? (
          <div className="flex items-center gap-x-2 pt-1">
            <Prompt symbol="$" className="text-[13px]" />
            <span aria-hidden className="blink inline-block w-[6px] h-[13px] bg-ink align-middle" />
          </div>
        ) : null}
      </div>

      <PlayerControls stepper={stepper} total={track.lines.length} label={multi ? track.label : undefined} />
    </div>
  )
}
