'use client'

import { motion } from 'motion/react'
import { IconArrowDown } from '@/components/icons/iconly'
import { useMotionPreference } from '@/hooks/use-motion-preference'
import { easeInOut } from '@/lib/motion'
import { links } from '@/lib/site'

/**
 * Eyebrow pill. One line from `sm` up. On phones it is a very small pill with a shortened label (the first words
 * of the same copy), so the headline gets the room.
 */
export function HeroEyebrow() {
  const reduce = useMotionPreference()

  return (
    <a
      href={links.docs}
      aria-label="Backends, agents, and devices: explore the docs"
      className="group relative inline-flex h-7 min-w-0 max-w-full items-center gap-1.5 rounded-full border pr-2 pl-2.5 font-medium font-sans text-[11px] text-foreground/80 leading-none transition-[border-color,color] duration-150 before:absolute before:inset-x-0 before:-inset-y-1.5 before:content-[''] hover:border-line-strong hover:text-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-4 motion-reduce:transition-none sm:h-8 sm:gap-2 sm:pr-2.5 sm:pl-3 sm:text-[13px]"
    >
      <span className="flex shrink-0 items-center justify-center">
        <svg viewBox="0 0 24 24" aria-hidden="true" className="size-3 shrink-0 fill-current sm:size-3.5">
          {[3, 10, 17].map((x, index) => (
            <motion.g
              key={x}
              initial={false}
              whileInView={reduce ? { opacity: 1 } : { opacity: [0.35, 1, 0.55, 1] }}
              viewport={{ once: true }}
              transition={{ duration: reduce ? 0 : 1.8, delay: reduce ? 0 : index * 0.18, ease: easeInOut }}
            >
              <rect x={x} y="3" width="4" height="4" />
              <rect x={x} y="10" width="4" height="12" />
            </motion.g>
          ))}
        </svg>
      </span>
      <span className="min-w-0 truncate sm:hidden">Open-source engine</span>
      <span className="hidden min-w-0 truncate sm:inline">Open-source engine for backends, agents, and devices</span>
      <IconArrowDown className="size-3.5 shrink-0 -rotate-90 text-muted-foreground transition-transform duration-150 group-hover:translate-x-0.5 motion-reduce:transition-none sm:size-4" />
    </a>
  )
}
