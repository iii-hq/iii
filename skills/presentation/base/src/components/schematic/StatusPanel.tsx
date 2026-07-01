import { cn } from '@lib/lib/utils'
import type * as React from 'react'

export type StatusVariant = 'info' | 'success' | 'warn' | 'alert'

const variantTone: Record<StatusVariant, { border: string; icon: string; headline: string }> = {
  info: {
    border: 'border-rule',
    icon: 'text-ink',
    headline: 'text-ink',
  },
  success: {
    border: 'border-accent',
    icon: 'text-accent',
    headline: 'text-accent',
  },
  warn: {
    border: 'border-warn',
    icon: 'text-warn',
    headline: 'text-warn',
  },
  alert: {
    border: 'border-alert',
    icon: 'text-alert',
    headline: 'text-alert',
  },
}

interface StatusPanelProps {
  variant?: StatusVariant
  icon?: React.ReactNode
  headline: React.ReactNode
  detail?: React.ReactNode
  className?: string
}

export function StatusPanel({ variant = 'info', icon, headline, detail, className }: StatusPanelProps) {
  const tone = variantTone[variant]
  return (
    <div className={cn('flex items-start gap-x-3 border bg-bg px-3.5 py-3', tone.border, className)}>
      {icon ? (
        <span aria-hidden className={cn('size-[18px] shrink-0', tone.icon)}>
          {icon}
        </span>
      ) : null}
      <div className="min-w-0 flex flex-col gap-y-0.5">
        <div className={cn('font-mono text-[13px] font-semibold lowercase', tone.headline)}>{headline}</div>
        {detail ? <div className="font-mono text-[12px] text-ink-faint lowercase">{detail}</div> : null}
      </div>
    </div>
  )
}
