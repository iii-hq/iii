'use client'

import { animate, useInView } from 'motion/react'
import { useEffect, useRef, useState } from 'react'

import { useMotionPreference } from '@/hooks/use-motion-preference'
import { easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

/** Seconds the figure takes to settle. Marketing figures may run past the 300ms UI budget. */
const settleDuration = 1.1

/**
 * Where a figure starts before it settles. Latencies start higher and come down; counts start from nothing.
 * Both are clamped to the final value's digit count (3.9 starts at 9.8, never 11.7; a two-digit count starts
 * at 10, not 0), so the figure never changes width while it is moving.
 */
function startFor(value: number, decimals: number, direction: 'down' | 'up') {
  const intDigits = Math.max(1, Math.floor(value).toString().length)
  if (direction === 'up') return intDigits === 1 ? 0 : 10 ** (intDigits - 1)
  const cap = 10 ** intDigits - 10 ** -decimals
  return Math.min(value * 2.5, cap)
}

/*
 * Geist Pixel figures are proportional (a "1" is a third narrower than a "4"), so a ticking number would
 * jitter and push its unit around. Each character sits centred in a fixed cell instead, wide enough for the
 * widest digit, which also makes the figure read like a dot-matrix readout. The first digit is flush left so
 * the figure lines up with the label beneath it.
 */
function Cells({ text }: { text: string }) {
  return text.split('').map((ch, i) => (
    <span
      // biome-ignore lint/suspicious/noArrayIndexKey: Characters are positional; the cell at index i is always the i-th glyph.
      key={i}
      className={cn(
        'inline-block',
        ch === '.' ? 'w-[0.2em] text-center' : 'w-[0.66em]',
        i === 0 ? 'text-left' : 'text-center',
      )}
    >
      {ch}
    </span>
  ))
}

type MetricFigureProps = {
  /** The figure as written in content, so decimals are kept exactly ("0.378", "1.9", "3"). */
  value: string
  unit: string | null
  /** Seconds before the figure starts moving. */
  delay?: number
  className?: string
}

/**
 * A benchmark figure that settles on its value the first time it scrolls into view. Latencies count
 * *down* to the result, so the motion reads as the engine getting faster rather than slower; plain counts
 * (the SDK languages) count up. The server renders the final value, so the number is right without
 * JavaScript, and fixed-width character cells keep it from shifting layout while it moves.
 */
export function MetricFigure({ value, unit, delay = 0, className }: MetricFigureProps) {
  const reduce = useMotionPreference()
  const ref = useRef<HTMLSpanElement>(null)
  const seen = useInView(ref, { once: true, margin: '0px 0px -10% 0px' })
  const [text, setText] = useState(value)

  useEffect(() => {
    if (reduce || !seen) return
    const decimals = value.split('.')[1]?.length ?? 0
    const target = Number(value)
    const from = startFor(target, decimals, unit ? 'down' : 'up')
    const controls = animate(from, target, {
      duration: settleDuration,
      delay,
      ease: easeOut,
      onUpdate: (v) => setText(v.toFixed(decimals)),
      onComplete: () => setText(value),
    })
    return () => controls.stop()
  }, [value, unit, delay, reduce, seen])

  return (
    <span ref={ref} className={className}>
      <span aria-hidden>
        <Cells text={text} />
      </span>
      <span className="sr-only">{value}</span>
      {unit ? <span className="ml-1 text-[0.5em] text-muted-foreground">{unit}</span> : null}
    </span>
  )
}
