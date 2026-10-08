'use client'

import { motion } from 'motion/react'

import { easeInOut } from '@/lib/motion'

/** The iii mark; on hover the three dots hop in sequence. */
export function NavLogo() {
  const dots = [0, 350.1, 700.21]
  return (
    <motion.svg
      viewBox="0 -140 933.61 1190.31"
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
            width="233.4"
            height="233.4"
            variants={{
              rest: { y: 0 },
              hover: { y: [0, -120, 0], transition: { duration: 0.5, delay: i * 0.07, ease: easeInOut } },
            }}
          />
          <rect x={x} y="350.1" width="233.4" height="700.21" />
        </g>
      ))}
    </motion.svg>
  )
}
