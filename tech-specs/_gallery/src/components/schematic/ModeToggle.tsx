import { cn } from '@/lib/utils'
import * as React from 'react'

interface ModeToggleOption<T extends string> {
  value: T
  label: React.ReactNode
}

interface ModeToggleProps<T extends string> {
  value: T
  onChange: (next: T) => void
  options: ModeToggleOption<T>[]
  className?: string
  /** accessible name for the toggle group, e.g. "theme" */
  label?: string
}

export function ModeToggle<T extends string>({
  value,
  onChange,
  options,
  className,
  label,
}: ModeToggleProps<T>) {
  // a group of toggle buttons (aria-pressed is valid on buttons). not a
  // tablist — there are no tabpanels — so role="tab"/aria-selected would be the
  // wrong semantics; an unnamed tablist also announces ambiguously.
  return (
    <div
      role="group"
      aria-label={label}
      className={cn('inline-flex border border-rule p-[2px]', className)}
    >
      {options.map((opt) => {
        const active = opt.value === value
        return (
          <button
            key={opt.value}
            type="button"
            aria-pressed={active}
            onClick={() => onChange(opt.value)}
            className={cn(
              'font-mono text-[13px] px-3 py-1 transition-colors lowercase cursor-pointer',
              active
                ? 'bg-ink text-bg'
                : 'bg-transparent text-ink-faint hover:text-ink',
            )}
          >
            {opt.label}
          </button>
        )
      })}
    </div>
  )
}
