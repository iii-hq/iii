import { cn } from '@lib/lib/utils'
import type * as React from 'react'

interface CodeBlockProps extends Omit<React.HTMLAttributes<HTMLPreElement>, 'title'> {
  title?: React.ReactNode
  children: React.ReactNode
}

/**
 * `bg` fill, 1px rule border, 12.5px mono. Optional `panel` head strip with a
 * label-caps title. Light syntax tinting is applied by the caller via
 * <K> / <S> / <C> helpers below — accent stays rationed to string literals
 * and the active return value.
 */
export function CodeBlock({ title, className, children, ...props }: CodeBlockProps) {
  return (
    <div className={cn('border border-rule bg-bg min-w-0', className)}>
      {title ? (
        <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
          {title}
        </div>
      ) : null}
      <pre className="overflow-x-auto px-5 py-4 font-mono text-[12.5px] leading-[1.55] text-ink" {...props}>
        <code>{children}</code>
      </pre>
    </div>
  )
}

/** keyword — bold ink */
export function K({ children }: { children: React.ReactNode }) {
  return <span className="font-semibold text-ink">{children}</span>
}

/** string literal — the accent moment of a code block */
export function S({ children }: { children: React.ReactNode }) {
  return <span className="text-accent">{children}</span>
}

/** comment — italic ghost */
export function C({ children }: { children: React.ReactNode }) {
  return <span className="italic text-ink-ghost">{children}</span>
}

/** muted punctuation / secondary tokens */
export function M({ children }: { children: React.ReactNode }) {
  return <span className="text-ink-faint">{children}</span>
}
