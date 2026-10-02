'use client'

import { animate } from 'motion/react'
import { useEffect, useState } from 'react'

import { useMotionPreference } from '@/hooks/use-motion-preference'
import { easeOut } from '@/lib/motion'

type Format = 'plain' | 'compact'

const formatters: Record<Format, (n: number) => string> = {
  plain: (n) => n.toLocaleString('en-US'),
  compact: (n) =>
    new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 }).format(n).toLowerCase(),
}

/**
 * A live count that ticks up to its value the first time it renders (last 10% of the way, 1.2s).
 * The server renders the final value, so the number is right without JavaScript and never shifts layout.
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
  const [text, setText] = useState(() => formatters[format](value))

  useEffect(() => {
    if (reduce) return
    const render = formatters[format]
    const controls = animate(Math.round(value * 0.9), value, {
      duration: 1.2,
      delay,
      ease: easeOut,
      onUpdate: (v) => setText(render(Math.round(v))),
    })
    return () => controls.stop()
  }, [value, format, delay, reduce])

  return <span className={className}>{text}</span>
}
