import * as React from 'react'
import { cn } from '@/lib/utils'

type ChipTone = 'ink' | 'faint' | 'ghost' | 'accent' | 'alert'

const toneClasses: Record<ChipTone, string> = {
  ink: 'border-rule text-ink',
  faint: 'border-rule text-ink-faint',
  ghost: 'border-rule-2 text-ink-ghost',
  accent: 'border-accent text-accent',
  alert: 'border-alert text-alert',
}

interface FnChipProps extends React.HTMLAttributes<HTMLSpanElement> {
  tone?: ChipTone
  children: React.ReactNode
}

/**
 * a function-id / trigger-type chip: 1px border, code-sm mono, no fill.
 * identifiers keep their original casing — the lowercase rule does not
 * apply to ids.
 */
export function FnChip({
  tone = 'faint',
  className,
  children,
  ...props
}: FnChipProps) {
  return (
    <span
      className={cn(
        'inline-flex items-center border bg-bg px-1.5 py-0.5 font-mono text-[11.5px] leading-[1.4] whitespace-nowrap transition-colors',
        toneClasses[tone],
        className,
      )}
      {...props}
    >
      {children}
    </span>
  )
}
