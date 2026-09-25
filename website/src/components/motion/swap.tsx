"use client"

import { AnimatePresence, m, useReducedMotion } from "motion/react"
import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

type SwapProps = {
  /** change this to swap the content (e.g. "idle" → "copied") */
  id: string | number
  children: ReactNode
  className?: string
}

/**
 * Crossfades content in place. A little blur bridges the two states so the eye
 * reads one element morphing rather than two overlapping (Emil Kowalski's trick).
 * `popLayout` takes the outgoing copy out of flow so the new one sizes the slot.
 */
export function Swap({ id, children, className }: SwapProps) {
  const reduce = useReducedMotion()
  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <m.span
        key={id}
        className={cn("inline-flex items-center gap-[inherit]", className)}
        initial={reduce ? { opacity: 0 } : { opacity: 0, y: 6, filter: "blur(4px)" }}
        animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
        exit={
          reduce
            ? { opacity: 0, transition: { duration: 0.1 } }
            : { opacity: 0, y: -6, filter: "blur(4px)", transition: { duration: 0.14, ease: [0.4, 0, 1, 1] } }
        }
        transition={{ duration: 0.22, ease: [0.23, 1, 0.32, 1] }}
      >
        {children}
      </m.span>
    </AnimatePresence>
  )
}
