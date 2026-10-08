'use client'

import type { CSSProperties } from 'react'

import type { CodeLine } from '@/lib/highlight'
import { cn } from '@/lib/utils'

type Props = {
  lines: CodeLine[]
  title: string
  /** Status shown at the right of the header. */
  status?: string
  /** Whole panel is the current subject. */
  lit?: boolean
  /** Lines to mark (0-based). */
  highlight?: number[]
  className?: string
}

/** Shiki-highlighted code (Vesper on dark) with an optional lit header and highlighted lines, in step with the graph. */
export function TokenCode({ lines, title, status, lit, highlight = [], className }: Props) {
  return (
    <div
      className={cn(
        'overflow-hidden rounded-xl border bg-card transition-colors duration-300',
        lit ? 'border-line-strong' : '',
        className,
      )}
    >
      <div className="flex h-10 items-center justify-between gap-3 border-b px-4">
        <span className="flex min-w-0 items-center gap-2 font-mono text-[13px]">
          <span className={cn('size-1.5 shrink-0 rounded-full', lit ? 'bg-ok' : 'bg-muted-foreground/40')} />
          <span className="truncate text-foreground">{title}</span>
        </span>
        {status ? <span className="shrink-0 font-sans text-[12px] text-muted-foreground">{status}</span> : null}
      </div>
      <pre className="overflow-x-auto px-3 py-3 font-mono text-[13px] leading-[1.7] [scrollbar-width:thin]">
        <code className="code-tokens">
          {lines.map((tokens, index) => (
            <span
              // biome-ignore lint/suspicious/noArrayIndexKey: Lines are static and never reorder.
              key={index}
              className={cn(
                'block px-1 transition-colors duration-300',
                highlight.includes(index) ? 'bg-faint shadow-[inset_2px_0_0_0_var(--hero-accent)]' : '',
              )}
            >
              <span className="whitespace-pre">
                {tokens.map((token, position) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: Tokens are static per line and never reorder.
                  <span key={position} style={token.style as CSSProperties}>
                    {token.text}
                  </span>
                ))}
                {'\n'}
              </span>
            </span>
          ))}
        </code>
      </pre>
    </div>
  )
}
