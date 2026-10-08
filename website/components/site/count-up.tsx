'use client'

import { animate, useInView } from 'motion/react'
import { useEffect, useRef, useState } from 'react'

import { useMotionPreference } from '@/hooks/use-motion-preference'
import { easeOut } from '@/lib/motion'

type Format = 'plain' | 'compact'

const formatters: Record<Format, (n: number) => string> = {
  plain: (n) => n.toLocaleString('en-US'),
  compact: (n) =>
    new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 }).format(n).toLowerCase(),
}

/**
 * A live count that ticks up to its value the first time it scrolls into view (last 10% of the way, 1.2s).
 * Waiting for the viewport means a count further down the page still moves when the reader gets there,
 * instead of finishing unseen on load. The server renders the final value, so the number is right without
 * JavaScript and never shifts layout.
 */
export function CountUp({
  value,
  format = 'plain',
  delay = 0.3,
  className,
}: {
  value: number
  format?: Format
  /** Seconds before the count starts moving. */
  delay?: number
  className?: string
}) {
  const reduce = useMotionPreference()
  const ref = useRef<HTMLSpanElement>(null)
  const seen = useInView(ref, { once: true, margin: '0px 0px -10% 0px' })
  const [text, setText] = useState(() => formatters[format](value))

  useEffect(() => {
    if (reduce || !seen) return
    const render = formatters[format]
    const controls = animate(Math.round(value * 0.9), value, {
      duration: 1.2,
      delay,
      ease: easeOut,
      onUpdate: (v) => setText(render(Math.round(v))),
    })
    return () => controls.stop()
  }, [value, format, delay, reduce, seen])

  return (
    <span ref={ref} className={className}>
      {text}
    </span>
  )
}
