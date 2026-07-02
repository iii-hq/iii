import { cn } from '@lib/lib/utils'
import type * as React from 'react'

interface CellProps {
  title?: React.ReactNode
  children: React.ReactNode
  className?: string
  bodyClassName?: string
}

export function Cell({ title, children, className, bodyClassName }: CellProps) {
  return (
    <div className={cn('border border-rule bg-bg p-7', className)}>
      {title ? (
        <div className="font-mono text-[16px] font-semibold tracking-[-0.01em] text-ink mb-3 lowercase">{title}</div>
      ) : null}
      <div className={cn('font-mono text-[13px] leading-[1.7] text-ink-faint', bodyClassName)}>{children}</div>
    </div>
  )
}
