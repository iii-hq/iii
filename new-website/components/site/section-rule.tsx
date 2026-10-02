'use client'

import { motion } from 'motion/react'

import { easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

const draw = { hidden: { scaleX: 0 }, shown: { scaleX: 1 } }

/**
 * A section's hairline top rule, drawn from left to right the first time it scrolls into view, in the same
 * direction the pixel heading below it prints. The unscaled wrapper is what gets observed (a zero-width
 * target never intersects); the inner line is what scales. Reduced motion: `.rule` in globals.css keeps it drawn.
 */
export function SectionRule({ className }: { className?: string }) {
  return (
    <motion.div
      aria-hidden
      className={cn('pointer-events-none absolute inset-x-0 top-0 h-px', className)}
      initial="hidden"
      whileInView="shown"
      viewport={{ once: true, amount: 1 }}
    >
      <motion.div
        className="rule h-px origin-left bg-border"
        variants={draw}
        transition={{ duration: 0.9, ease: easeOut }}
      />
    </motion.div>
  )
}
