import { cn } from '@lib/lib/utils'
import type * as React from 'react'

interface PromptProps {
  symbol?: string
  className?: string
  children?: React.ReactNode
}

export function Prompt({ symbol = '$', className, children }: PromptProps) {
  return (
    <span className={cn('font-mono text-accent', className)}>
      {symbol}
      {children !== undefined ? <span className="text-ink ml-2">{children}</span> : null}
    </span>
  )
}
