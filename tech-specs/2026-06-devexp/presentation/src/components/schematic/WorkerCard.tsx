import * as React from 'react'
import { cn } from '@/lib/utils'

interface WorkerCardProps {
  name: string
  version?: string
  description: React.ReactNode
  command: React.ReactNode
  kind: string
  focused?: boolean
  className?: string
}

export function WorkerCard({
  name,
  version,
  description,
  command,
  kind,
  focused,
  className,
}: WorkerCardProps) {
  return (
    <article
      className={cn(
        'border border-rule transition-colors',
        focused ? 'bg-panel border-l-2 border-l-accent' : 'bg-bg',
        className,
      )}
    >
      <header className="flex items-center justify-between px-4 py-3 border-b border-rule-2">
        <div className="font-mono text-[16px] font-semibold lowercase text-ink">
          {name}
        </div>
        {version ? (
          <div className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost tabular-nums">
            v{version}
          </div>
        ) : null}
      </header>
      <div className="px-4 py-3 font-mono text-[13px] leading-[1.7] text-ink-faint">
        {description}
      </div>
      <div className="bg-panel font-mono text-[12.5px] text-ink px-4 py-2 border-t border-rule-2">
        {command}
      </div>
      <footer className="flex items-center justify-between px-4 py-2 border-t border-rule-2">
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint">
          {kind}
        </span>
        <span aria-hidden className="text-accent">
          ✓
        </span>
      </footer>
    </article>
  )
}
