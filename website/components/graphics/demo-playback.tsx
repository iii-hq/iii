'use client'

import { IconPause, IconPlay } from '@/components/icons/iconly'

export function DemoPlayback({ paused, reduce, onToggle }: { paused: boolean; reduce: boolean; onToggle: () => void }) {
  if (reduce)
    return (
      <span className="inline-flex min-h-11 shrink-0 items-center font-mono text-[12px] text-muted-foreground">
        Motion off
      </span>
    )
  return (
    <button
      type="button"
      onClick={onToggle}
      aria-label={paused ? 'Play animation' : 'Pause animation'}
      className="pressable inline-flex min-h-11 shrink-0 items-center gap-2 rounded-md px-3 text-xs text-muted-foreground outline-none hover:bg-faint hover:text-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2"
    >
      {paused ? <IconPlay className="size-3.5" /> : <IconPause className="size-3.5" />}
      {paused ? 'Play' : 'Pause'}
    </button>
  )
}
