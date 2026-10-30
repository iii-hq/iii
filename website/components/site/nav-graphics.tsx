'use client'

import { motion } from 'motion/react'

import { easeInOut } from '@/lib/motion'

/** The iii mark; on hover the three dots hop in sequence. */
export function NavLogo() {
  const dots = [0, 403.4, 806.81]
  return (
    <motion.svg
      viewBox="0 -160 1075.74 1235.74"
      aria-hidden
      className="h-[18px] w-auto fill-foreground"
      initial="rest"
      animate="rest"
      whileHover="hover"
      whileFocus="hover"
    >
      {dots.map((x, i) => (
        <g key={x}>
          <motion.rect
            x={x}
            y="0.05"
            width="268.94"
            height="268.94"
            variants={{
              rest: { y: 0.05 },
              hover: { y: [0.05, -140, 0.05], transition: { duration: 0.5, delay: i * 0.07, ease: easeInOut } },
            }}
          />
          <rect x={x} y="403.45" width="268.94" height="672.24" />
        </g>
      ))}
    </motion.svg>
  )
}
