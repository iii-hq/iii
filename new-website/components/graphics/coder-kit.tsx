'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { easeInOut, easeOut } from '@/lib/motion'

import { cn } from '@/lib/utils'
import { useGraphicLoop } from './use-graphic-loop'

export const ease = easeOut

/** Shared viewBox for the five CODER graphics. */
export const VIEW = { w: 460, h: 340 }

/**
 * Scripted step machine for a CODER graphic: counts 0..count-1 every `stepMs` while
 * `active`, and returns `staticStep` (the complete, readable state) otherwise.
 */
export function useCoderSteps(count: number, stepMs: number, staticStep = count - 1) {
  const loop = useGraphicLoop<HTMLDivElement>()
  const [tick, setTick] = useState(0)

  useEffect(() => {
    if (!loop.active) return
    setTick(0)
    const id = window.setInterval(() => setTick((t) => t + 1), stepMs)
    return () => window.clearInterval(id)
  }, [loop.active, stepMs])

  return {
    ...loop,
    step: loop.active ? tick % count : staticStep,
    /** Increments each time the script restarts; key animated groups by it. */
    cycle: Math.floor(tick / count),
  }
}

type FrameProps = {
  frameRef: React.Ref<HTMLDivElement>
  label: string
  caption: string
  captionKey: string | number
  children: React.ReactNode
  className?: string
}

/** SVG wrapper with an HTML caption underneath (stays legible at any width). */
export function CoderFrame({ frameRef, label, caption, captionKey, children, className }: FrameProps) {
  return (
    <div ref={frameRef} className={cn('relative', className)}>
      <svg viewBox={`0 0 ${VIEW.w} ${VIEW.h}`} role="img" aria-label={label} className="h-auto w-full overflow-visible">
        {children}
      </svg>
      <div
        aria-hidden
        className="mt-3 flex h-5 items-center justify-center overflow-hidden text-center font-mono text-[12px] text-muted-foreground"
      >
        <AnimatePresence mode="wait" initial={false}>
          <motion.span
            key={captionKey}
            className="truncate"
            initial={{ opacity: 0, y: 6, filter: 'blur(2px)' }}
            animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
            exit={{ opacity: 0, y: -6, filter: 'blur(2px)' }}
            transition={{ duration: 0.3, ease }}
          >
            {caption}
          </motion.span>
        </AnimatePresence>
      </div>
    </div>
  )
}

/** Tiny 18×18 kind chip (language / worker type). */
export function KindChip({ x, y, kind, size = 18 }: { x: number; y: number; kind: string; size?: number }) {
  return (
    <g>
      <rect x={x} y={y} width={size} height={size} rx={5} fill="var(--faint)" stroke="var(--line)" />
      <text
        x={x + size / 2}
        y={y + size / 2 + 3}
        textAnchor="middle"
        className="fill-muted-foreground font-mono"
        fontSize={size > 16 ? 8.5 : 7.5}
      >
        {kind}
      </text>
    </g>
  )
}

/** Small mono uppercase label used for panel headers inside graphics. */
export function SvgLabel({
  x,
  y,
  children,
  anchor = 'start',
}: {
  x: number
  y: number
  children: React.ReactNode
  anchor?: 'start' | 'middle' | 'end'
}) {
  return (
    <text
      x={x}
      y={y}
      textAnchor={anchor}
      className="fill-muted-foreground font-mono"
      fontSize={9}
      letterSpacing="0.12em"
    >
      {children}
    </text>
  )
}

type Point = { x: number; y: number }

/** A packet travelling along a straight segment. */
export function Packet({
  from,
  to,
  delay = 0,
  duration = 0.6,
}: {
  from: Point
  to: Point
  delay?: number
  duration?: number
}) {
  return (
    <motion.circle
      r={3}
      fill="var(--foreground)"
      initial={{ cx: from.x, cy: from.y, opacity: 0 }}
      animate={{ cx: to.x, cy: to.y, opacity: [0, 1, 1, 0] }}
      transition={{
        cx: { duration, delay, ease: easeInOut },
        cy: { duration, delay, ease: easeInOut },
        opacity: { duration, delay, times: [0, 0.15, 0.85, 1] },
      }}
    />
  )
}

/** A foreground segment drawn with pathLength, fading to a trace. */
export function RouteLine({
  from,
  to,
  delay = 0,
  duration = 0.6,
}: {
  from: Point
  to: Point
  delay?: number
  duration?: number
}) {
  return (
    <motion.line
      x1={from.x}
      y1={from.y}
      x2={to.x}
      y2={to.y}
      stroke="var(--hero-accent)"
      strokeWidth={1.25}
      initial={{ pathLength: 0, opacity: 0.9 }}
      animate={{ pathLength: 1, opacity: [0.9, 0.9, 0.3] }}
      transition={{
        pathLength: { duration, delay, ease: easeInOut },
        opacity: { duration: duration + 1, delay, times: [0, 0.6, 1] },
      }}
    />
  )
}

/** A status dot (ok / warn / fail) that pops in. */
export function StatusDot({
  cx,
  cy,
  tone = 'ok',
  delay = 0,
  r = 2.75,
}: {
  cx: number
  cy: number
  tone?: 'ok' | 'warn' | 'fail'
  delay?: number
  r?: number
}) {
  return (
    <motion.circle
      cx={cx}
      cy={cy}
      r={r}
      fill={`var(--${tone})`}
      initial={{ opacity: 0, scale: 0.4 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.35, delay, ease }}
      style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
    />
  )
}
