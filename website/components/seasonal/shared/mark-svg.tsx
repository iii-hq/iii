'use client'

import { cn } from '@/lib/utils'

/*
 * Shared pieces for the seasonal iii marks, drawn on the mark's own 16-unit grid: three columns 4 wide with 2-unit
 * gaps, a dot 4 tall, a gap of 2, a stem 10 tall. At rest (t = 0) every seasonal mark draws exactly the plain mark.
 */

export type MarkProps = {
  on: boolean
  /** The mark's size in px; the drawing may spill past it. */
  size: number
  /** Freeze on the frame a reduced-motion reader would see. */
  still?: boolean
}

/** oklch(0.97 0 0), the dark theme's foreground. */
export const INK = '#f5f5f5'

export const COLS = [0, 1, 2] as const
export const colX = (col: number) => col * 6

/** An SVG canvas whose 0..16 box is exactly `size` px, with `pad` units of room on every side for things to spill. */
export function MarkSvg({
  size,
  pad = 6,
  className,
  style,
  children,
}: {
  size: number
  pad?: number
  className?: string
  style?: React.CSSProperties
  children: React.ReactNode
}) {
  const box = 16 + pad * 2
  const offset = `${(-pad / 16) * 100}%`
  const extent = `${(box / 16) * 100}%`
  return (
    <div aria-hidden className={cn('relative shrink-0', className)} style={{ width: size, height: size, ...style }}>
      <svg
        aria-hidden="true"
        viewBox={`${-pad} ${-pad} ${box} ${box}`}
        className="pointer-events-none absolute overflow-visible"
        style={{ left: offset, top: offset, width: extent, height: extent }}
      >
        {children}
      </svg>
    </div>
  )
}
