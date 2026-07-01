import { Prompt } from '@lib/components/schematic/Prompt'
import type * as React from 'react'

interface UseCaseShellProps {
  eyebrow: string
  title: string
  description: React.ReactNode
  children: React.ReactNode
}

const OTHERS: Array<{ id: string; href: string; label: string }> = [
  { id: 'telegram', href: '#/telegram', label: 'telegram bot' },
  { id: 'console', href: '#/console', label: 'console chat' },
  { id: 'loops', href: '#/loops', label: 'agentic loops' },
]

export function UseCaseShell({ eyebrow, title, description, children }: UseCaseShellProps) {
  return (
    <main>
      <div className="px-4 py-12 @3xl:px-9 @3xl:py-16 border-b border-rule">
        <div className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint mb-4">
          <Prompt symbol="$">use case — {eyebrow}</Prompt>
        </div>
        <h1 className="font-mono text-[30px] @3xl:text-[40px] font-medium leading-[1.15] tracking-[-0.02em] text-ink lowercase max-w-[26ch]">
          {title}
        </h1>
        <p className="mt-4 font-mono text-[14px] leading-[1.7] text-ink-faint lowercase max-w-[64ch]">{description}</p>
      </div>

      <div className="px-4 py-12 @3xl:px-9 flex flex-col gap-10">{children}</div>

      <div className="border-t border-rule px-4 py-8 @3xl:px-9 flex flex-wrap items-center gap-x-6 gap-y-3">
        <span className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint">more walkthroughs</span>
        {OTHERS.map((other) => (
          <a
            key={other.id}
            href={other.href}
            className="font-mono text-[13px] lowercase text-ink-faint hover:text-ink transition-colors"
          >
            {other.label} →
          </a>
        ))}
        <a href="#/" className="ml-auto font-mono text-[13px] lowercase text-ink hover:text-accent transition-colors">
          ← back to the overview
        </a>
      </div>
    </main>
  )
}
