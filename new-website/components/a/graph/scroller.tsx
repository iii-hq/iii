import type { CSSProperties } from 'react'

import { cn } from '@/lib/utils'

type GraphScrollerProps = {
  children: React.ReactNode
  className?: string
  /** Narrowest the graph may render below `lg`; 720px keeps the graph's 16-unit mono at about 11px. */
  minWidth?: number
}

/**
 * Below `lg` the circuit graphs cannot shrink to a phone column without their type dropping under 5px, so they
 * keep a readable width and scroll sideways instead: a full-bleed strip (the page container is 90% wide, so the
 * strip takes the remaining 5vw on each side) with its edges faded as the hint that there is more. From `lg` the
 * wrapper is inert and the graph fills its column as before.
 */
export function GraphScroller({ children, className, minWidth = 720 }: GraphScrollerProps) {
  return (
    <div
      style={{ '--graph-min': `${minWidth}px` } as CSSProperties}
      className={cn(
        '-mx-[5vw] overflow-x-auto overscroll-x-contain px-[5vw] py-3 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden',
        '[mask-image:linear-gradient(to_right,transparent,#000_5vw,#000_calc(100%-5vw),transparent)]',
        'lg:mx-0 lg:overflow-visible lg:px-0 lg:py-0 lg:[mask-image:none]',
        className,
      )}
    >
      <div className="min-w-[var(--graph-min)] lg:min-w-0">{children}</div>
    </div>
  )
}
