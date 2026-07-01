import * as React from 'react'
import { cn } from '@/lib/utils'

interface SpecSheetProps {
  title: React.ReactNode
  meta?: React.ReactNode
  children: React.ReactNode
  defaultOpen?: boolean
  className?: string
}

/**
 * the expandable technical layer: a native <details> datasheet with a
 * panel-tinted head strip. closed by default so the page stays a story;
 * the spec is one click away.
 */
export function SpecSheet({
  title,
  meta,
  children,
  defaultOpen,
  className,
}: SpecSheetProps) {
  return (
    <details
      className={cn('group border border-rule bg-bg', className)}
      open={defaultOpen}
    >
      <summary className="flex cursor-pointer select-none items-center justify-between gap-x-3 bg-panel px-3.5 py-2.5 border-b border-transparent group-open:border-rule list-none [&::-webkit-details-marker]:hidden">
        <span className="flex items-center gap-x-2.5 min-w-0">
          <span
            aria-hidden
            className="font-mono text-[13px] text-accent w-3 shrink-0"
          >
            <span className="group-open:hidden">+</span>
            <span className="hidden group-open:inline">−</span>
          </span>
          <span className="font-mono text-[12px] font-medium uppercase tracking-[0.14em] text-ink-faint truncate">
            {title}
          </span>
        </span>
        {meta ? (
          <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost shrink-0">
            {meta}
          </span>
        ) : null}
      </summary>
      <div className="p-5">{children}</div>
    </details>
  )
}

interface SpecRowProps {
  name: string
  type?: string
  children?: React.ReactNode
  required?: boolean
  indent?: boolean
}

/** one row of a type-table: `name  type  required-marker` + faint description */
export function SpecRow({ name, type, children, required, indent }: SpecRowProps) {
  return (
    <div
      className={cn(
        'py-1.5 border-b border-rule-2 last:border-b-0',
        indent && 'pl-4 border-l border-rule-2',
      )}
    >
      <div className="flex flex-wrap items-baseline gap-x-3">
        <span className="font-mono text-[12.5px] text-ink">
          {name}
          {required ? <span className="text-accent">*</span> : null}
        </span>
        {type ? (
          <span className="font-mono text-[11.5px] text-ink-ghost">{type}</span>
        ) : null}
      </div>
      {children ? (
        <div className="mt-0.5 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">
          {children}
        </div>
      ) : null}
    </div>
  )
}
