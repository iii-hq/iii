import { Button } from '@lib/components/schematic/Button'
import type { Stepper } from '@lib/hooks/useStepper'
import { cn } from '@lib/lib/utils'

interface PlayerControlsProps {
  stepper: Stepper
  total: number
  label?: string
  className?: string
}

/** play / step / reset strip shared by every diagram player */
export function PlayerControls({ stepper, total, label, className }: PlayerControlsProps) {
  return (
    <div
      className={cn(
        'flex flex-wrap items-center gap-x-2 gap-y-2 border-t border-rule bg-panel px-3.5 py-2.5',
        className,
      )}
    >
      <Button variant="pill" size="sm" onClick={stepper.toggle}>
        {stepper.playing ? 'pause' : stepper.atEnd ? 'replay' : 'play'}
      </Button>
      <Button variant="icon" size="icon" aria-label="previous step" onClick={stepper.prev}>
        ←
      </Button>
      <Button variant="icon" size="icon" aria-label="next step" onClick={stepper.next}>
        →
      </Button>
      <Button variant="ghost" size="sm" onClick={stepper.reset}>
        reset
      </Button>
      <span className="ml-auto font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint tabular-nums">
        {label ? `${label} — ` : ''}step {stepper.step + 1}/{total}
      </span>
    </div>
  )
}
