'use client'

import { motion } from 'motion/react'

import { spotlightBaseClass, spotlightLayerClass, spotlightMove } from '@/components/site/spotlight'
import { duration, easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

/** Always two lines: the size scales with the viewport so "your agents can see" fits on one line down to 320px. */
const lines = ['The backend', 'your agents can see']

function Lines() {
  return lines.map((line) => (
    <span key={line} className="block whitespace-nowrap">
      {line}
    </span>
  ))
}

/**
 * Pixel headline in two lines at every width. It prints in from left to right on load, like a dot-matrix head passing
 * over the line. On hover the dots dim and a lit copy shows through a soft circle that follows the pointer, so the dots
 * under the cursor light up.
 */
export function HeroTitle() {
  return (
    /* The spotlight copy sits beside the heading, not inside it, so the h1 holds its text once (crawlers read
       aria-hidden text too). The wrapper carries the type and the pointer handler; the h1 inherits both. */
    <div
      onPointerMove={spotlightMove}
      className="group/title relative font-normal font-pixel text-[clamp(1.75rem,9.4vw,2.75rem)] leading-[1.06] tracking-[-0.02em] sm:text-[clamp(2.25rem,5vw,4.25rem)] sm:leading-[1.04]"
    >
      {/* biome-ignore lint/correctness/useUniqueElementIds: The page has one primary heading. */}
      <h1 id="hero-title" className="font-normal">
        <motion.span
          className={cn('print block', spotlightBaseClass)}
          initial={{ clipPath: 'inset(-8px 100% -8px 0px)' }}
          animate={{ clipPath: 'inset(-8px 0% -8px 0px)' }}
          transition={{ duration: duration.print + 0.2, delay: 0.15, ease: easeOut }}
        >
          <Lines />
        </motion.span>
      </h1>
      <span aria-hidden className={spotlightLayerClass}>
        <Lines />
      </span>
    </div>
  )
}
