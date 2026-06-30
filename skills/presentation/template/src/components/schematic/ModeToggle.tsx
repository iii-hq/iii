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
}

export function ModeToggle<T extends string>({
  value,
  onChange,
  options,
  className,
}: ModeToggleProps<T>) {
  return (
    <div
      role="tablist"
      className={cn('inline-flex border border-rule p-[2px]', className)}
    >
      {options.map((opt) => {
        const active = opt.value === value
        return (
          <button
            key={opt.value}
            type="button"
            role="tab"
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
