'use client'

import { motion } from 'motion/react'

import { duration, easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

type PixelHeadingProps = {
  id?: string
  as?: 'h1' | 'h2'
  className?: string
  /** Seconds to wait once in view before printing. */
  delay?: number
  children: React.ReactNode
}

/*
 * Clipped 8px above and below so ascenders and descenders are never cut while the edge sweeps across.
 * The clip lives on an inner span: Chrome's IntersectionObserver honours the target's own clip-path, so a
 * heading clipped to zero width would never count as "in view". The heading is observed, the span is clipped.
 */
const print = {
  hidden: { clipPath: 'inset(-8px 100% -8px 0px)' },
  shown: { clipPath: 'inset(-8px 0% -8px 0px)' },
}

/**
 * Geist Pixel heading that prints in from left to right, like a dot-matrix head passing over the line.
 * The pointer spotlight is reserved for the hero. Reduced motion: `.print` in globals.css removes the clip.
 */
export function PixelHeading({ id, as = 'h2', className, delay = 0.1, children }: PixelHeadingProps) {
  const Component = motion[as]
  return (
    <Component
      id={id}
      className={cn('relative text-balance font-normal font-pixel', className)}
      initial="hidden"
      whileInView="shown"
      viewport={{ once: true, amount: 0.6 }}
    >
      <motion.span
        className="print block"
        variants={print}
        transition={{ duration: duration.print, delay, ease: easeOut }}
      >
        {children}
      </motion.span>
    </Component>
  )
}
