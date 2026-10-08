'use client'

import { motion } from 'motion/react'

import { duration, easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

type RevealProps = {
  children: React.ReactNode
  className?: string
  delay?: number
  as?: 'div' | 'li' | 'section' | 'header'
}

/**
 * Fades content up once, the first time it enters the viewport. The trigger is the element's leading edge
 * crossing the lower 10% line rather than a share of its area, so a phone column taller than the screen
 * reveals as soon as it arrives instead of sitting blank until a quarter of it is scrolled past.
 */
export function Reveal({ children, className, delay = 0, as = 'div' }: RevealProps) {
  const Component = motion[as]
  return (
    <Component
      className={cn('reveal', className)}
      /* A full transform string, not Motion's `y` shorthand: it stays hardware accelerated while the page is busy
         loading and scrolling, which is exactly when reveals run. */
      initial={{ opacity: 0, transform: 'translateY(10px)' }}
      whileInView={{ opacity: 1, transform: 'translateY(0px)' }}
      viewport={{ once: true, amount: 'some', margin: '0px 0px -10% 0px' }}
      transition={{ duration: duration.reveal, delay, ease: easeOut }}
    >
      {children}
    </Component>
  )
}
