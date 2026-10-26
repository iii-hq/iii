'use client'

import { animate, useInView } from 'motion/react'
import { useEffect, useRef, useState } from 'react'

import { useMotionPreference } from '@/hooks/use-motion-preference'
import { bytes, compact, type Formatted, micros } from '@/lib/benchmarks/format'
import { easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

import { useProfile } from './profile'

export type FigureFormat = 'perSec' | 'micros' | 'bytes'

/** One figure of the summary grid, for one machine size. */
export interface Figure {
  id: string
  label: string
  /** `null` when this size did not measure it. */
  value: number | null
  format: FigureFormat
  detail: string
}

const formats: Record<FigureFormat, (v: number) => Formatted> = {
  perSec: (v) => {
    const c = compact(v)
    return { value: c.value, unit: `${c.unit}/s` }
  },
  micros,
  bytes,
}

/**
 * The page's summary: six figures for the selected machine size, in a hairline grid like the homepage's
 * numbers. Switching sizes moves every figure from its old value to the new one; the label and detail swap
 * with it.
 */
export function Figures({ figures }: { figures: Record<string, Figure[]> }) {
  const { selected } = useProfile()
  const shown = figures[selected] ?? []
  return (
    <dl className="grid grid-cols-1 gap-px overflow-hidden rounded-xl border bg-border sm:grid-cols-2 lg:grid-cols-3">
      {shown.map((f, i) => (
        <div key={f.id} className="flex min-w-0 flex-col bg-card px-5 py-6 sm:px-7 sm:py-8">
          <dt className="order-2 mt-5 text-[14px] text-foreground leading-snug sm:text-[15px]">{f.label}</dt>
          <dd className="order-1 font-pixel text-[38px] text-foreground leading-none tabular-nums sm:text-[44px] lg:text-[50px]">
            {f.value === null ? (
              <span className="text-muted-foreground">—</span>
            ) : (
              <Tween value={f.value} format={formats[f.format]} delay={0.25 + i * 0.05} />
            )}
          </dd>
          <dd className="order-3 mt-1.5 min-h-[2.6em] text-pretty text-[13px] text-muted-foreground leading-snug">
            {f.detail}
          </dd>
        </div>
      ))}
    </dl>
  )
}

/**
 * A number that settles on its value: up from a little under it the first time it scrolls into view, then from
 * wherever it is to each new value. The server renders the final value, so it reads right without JavaScript.
 */
function Tween({ value, format, delay }: { value: number; format: (v: number) => Formatted; delay: number }) {
  const reduce = useMotionPreference()
  const ref = useRef<HTMLSpanElement>(null)
  const seen = useInView(ref, { once: true, margin: '0px 0px -10% 0px' })
  const [current, setCurrent] = useState(value)
  const from = useRef<number | null>(null)

  useEffect(() => {
    if (reduce) {
      setCurrent(value)
      return
    }
    if (!seen) return
    const start = from.current ?? value * 0.6
    const controls = animate(start, value, {
      duration: from.current === null ? 1.1 : 0.6,
      delay: from.current === null ? delay : 0,
      ease: easeOut,
      onUpdate: (v) => {
        from.current = v
        setCurrent(v)
      },
    })
    return () => controls.stop()
  }, [value, seen, reduce, delay])

  const f = format(current)
  const final = format(value)
  return (
    <span ref={ref}>
      <span aria-hidden>
        <Cells text={f.value} />
        <span className="ml-1.5 text-[0.42em] text-muted-foreground">{f.unit}</span>
      </span>
      <span className="sr-only">
        {final.value} {final.unit}
      </span>
    </span>
  )
}

/** Pixel digits in fixed cells, so a moving number never changes width (as the homepage's figures). */
function Cells({ text }: { text: string }) {
  return text.split('').map((ch, i) => (
    <span
      // biome-ignore lint/suspicious/noArrayIndexKey: Characters are positional.
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
