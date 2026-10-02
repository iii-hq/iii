'use client'

import { CopyButton } from '@/components/animate-ui/components/buttons/copy'
import { cn } from '@/lib/utils'

type CopyCommandProps = {
  command: string
  className?: string
}

/** A single shell command in a mono pill with an animated copy button. */
export function CopyCommand({ command, className }: CopyCommandProps) {
  return (
    <div
      className={cn(
        'group flex h-11 w-full min-w-0 items-center gap-3 rounded-lg border bg-card pr-1.5 pl-4 font-mono text-[13px]',
        className,
      )}
    >
      <span aria-hidden className="select-none text-muted-foreground">
        $
      </span>
      <code className="min-w-0 flex-1 overflow-x-auto whitespace-nowrap [scrollbar-width:none]">{command}</code>
      <CopyButton
        content={command}
        variant="ghost"
        size="sm"
        aria-label="Copy command"
        className="text-muted-foreground hover:text-foreground"
      />
    </div>
  )
}
