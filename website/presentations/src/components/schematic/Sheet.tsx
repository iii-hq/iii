import { cn } from '@lib/lib/utils'
import type * as React from 'react'

interface SheetProps {
  children: React.ReactNode
  className?: string
}

export function Sheet({ children, className }: SheetProps) {
  return (
    <div className={cn('mx-auto w-full max-w-[1200px] border-x border-rule min-h-screen bg-bg', className)}>
      {children}
    </div>
  )
}
