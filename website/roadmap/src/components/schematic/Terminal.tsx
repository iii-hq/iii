import { cn } from '@lib/lib/utils'
import type * as React from 'react'
import { Caret } from './Caret'
import { Prompt } from './Prompt'

interface TerminalProps {
  title?: React.ReactNode
  children: React.ReactNode
  className?: string
}

export function Terminal({ title, children, className }: TerminalProps) {
  return (
    <div className={cn('border border-rule bg-bg', className)}>
      {title ? (
        <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
          {title}
        </div>
      ) : null}
      <div className="p-4 font-mono text-[13px] text-ink">{children}</div>
    </div>
  )
}

interface TerminalRowProps {
  command: React.ReactNode
  output?: React.ReactNode
  showCaret?: boolean
  className?: string
}

export function TerminalRow({ command, output, showCaret, className }: TerminalRowProps) {
  return (
    <div className={cn('flex flex-col gap-y-1', className)}>
      <div className="flex items-center gap-x-2">
        <Prompt symbol="$" />
        <span className="text-ink">{command}</span>
        {showCaret ? <Caret /> : null}
      </div>
      {output ? <div className="pl-4 text-[12.5px] text-ink-faint">{output}</div> : null}
    </div>
  )
}
