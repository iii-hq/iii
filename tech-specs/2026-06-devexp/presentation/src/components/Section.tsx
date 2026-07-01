import * as React from 'react'
import { useEffect, useRef, useState } from 'react'
import { cn } from '@/lib/utils'

interface SectionProps {
  id: string
  index: string // drafting-sheet section number, e.g. "01"
  eyebrow: string
  title: React.ReactNode
  lede?: React.ReactNode
  children: React.ReactNode
  className?: string
}

/**
 * a numbered drafting-sheet section: full-width top rule, label-caps eyebrow
 * with the sheet number on the right, headline-section title, ink-faint lede.
 * content reveals on first scroll into view.
 */
export function Section({
  id,
  index,
  eyebrow,
  title,
  lede,
  children,
  className,
}: SectionProps) {
  const ref = useRef<HTMLElement>(null)
  const [shown, setShown] = useState(false)

  useEffect(() => {
    const el = ref.current
    if (!el) return
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setShown(true)
            observer.disconnect()
          }
        }
      },
      { rootMargin: '0px 0px -10% 0px' },
    )
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  return (
    <section
      id={id}
      ref={ref}
      className={cn('border-t border-rule scroll-mt-[64px]', className)}
    >
      <div className="flex items-center justify-between px-4 py-2.5 border-b border-rule-2 @3xl:px-9">
        <span className="font-mono text-[12px] font-medium uppercase tracking-[0.18em] text-ink-faint">
          {eyebrow}
        </span>
        <span className="font-mono text-[12px] font-medium uppercase tracking-[0.14em] text-ink-ghost tabular-nums">
          {index}
        </span>
      </div>
      <div
        className={cn(
          'px-4 py-12 @3xl:px-9 @3xl:py-16 reveal',
          shown && 'reveal-in',
        )}
      >
        <h2 className="font-mono text-[28px] font-medium leading-[1.25] tracking-[-0.01em] text-ink lowercase max-w-[28ch]">
          {title}
        </h2>
        {lede ? (
          <p className="mt-4 font-mono text-[14px] leading-[1.7] text-ink-faint lowercase max-w-[62ch]">
            {lede}
          </p>
        ) : null}
        <div className="mt-10">{children}</div>
      </div>
    </section>
  )
}
