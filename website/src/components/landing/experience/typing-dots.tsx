"use client"

import { m } from "motion/react"

const DOTS = [0, 1, 2]

/** Three dots that breathe in turn while the next step is "being typed". Opacity only, no bounce. */
export function TypingDots() {
  return (
    <m.div
      aria-hidden="true"
      className="ml-10 flex h-7 items-center gap-1 px-1"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.2, ease: "easeOut" }}
    >
      {DOTS.map((i) => (
        <m.span
          key={i}
          className="inline-block size-1.5 rounded-full bg-gray-9"
          initial={{ opacity: 0.35 }}
          animate={{ opacity: [0.35, 1, 0.35] }}
          transition={{ duration: 1.2, ease: "easeInOut", repeat: Infinity, delay: i * 0.15 }}
        />
      ))}
    </m.div>
  )
}
