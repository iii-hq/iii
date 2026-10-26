'use client'

import { useInView } from 'motion/react'
import { useId, useRef, useState } from 'react'

import { useMotionPreference } from '@/hooks/use-motion-preference'
import { cn } from '@/lib/utils'

const PAD_X = 4
const PAD_Y = 5
const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)'

/**
 * A benchmark's history as a small area chart, the registry's installs chart at row size: the line draws in
 * the first time it scrolls into view, the end dot lands on the latest release, and hovering scrubs across
 * releases with the value in a pill above. Lower is better for a timing, so the line falling is the engine
 * getting faster.
 */
export function Sparkline({
  values,
  labels,
  format,
  width: W = 140,
  height: H = 32,
  className,
}: {
  values: number[]
  /** One per value: the release it was measured at. */
  labels: string[]
  format: (v: number) => string
  width?: number
  height?: number
  className?: string
}) {
  const ref = useRef<SVGSVGElement>(null)
  const seen = useInView(ref, { once: true, margin: '0px 0px -10% 0px' })
  const reduce = useMotionPreference()
  const drawn = reduce || seen
  const [active, setActive] = useState<number | null>(null)
  const gradient = `spark-${useId().replace(/[^a-zA-Z0-9_-]/g, '')}`

  if (values.length < 2) return <span className={cn('block', className)} style={{ height: H }} />

  const max = Math.max(...values)
  const min = Math.min(...values)
  const range = max - min || max || 1
  const step = (W - PAD_X * 2) / (values.length - 1)
  const points = values.map((v, i) => [PAD_X + i * step, H - PAD_Y - ((v - min) / range) * (H - PAD_Y * 2)] as const)
  const line = points.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)},${y.toFixed(1)}`).join(' ')
  const area = `${line} L${points[points.length - 1][0]},${H} L${points[0][0]},${H} Z`
  const index = active ?? values.length - 1
  const [dotX, dotY] = points[index]

  return (
    <div className={cn('relative min-w-0', className)}>
      <svg
        ref={ref}
        viewBox={`0 0 ${W} ${H}`}
        aria-hidden="true"
        onPointerMove={(e) => {
          const rect = e.currentTarget.getBoundingClientRect()
          const x = ((e.clientX - rect.left) / rect.width) * W
          setActive(Math.min(values.length - 1, Math.max(0, Math.round((x - PAD_X) / step))))
        }}
        onPointerLeave={() => setActive(null)}
        className="block h-auto w-full overflow-visible text-foreground"
      >
        <defs>
          <linearGradient id={gradient} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="currentColor" stopOpacity="0.16" />
            <stop offset="100%" stopColor="currentColor" stopOpacity="0" />
          </linearGradient>
        </defs>
        <path
          d={area}
          fill={`url(#${gradient})`}
          style={{ opacity: drawn ? 1 : 0, transition: reduce || !drawn ? 'none' : `opacity 500ms ${EASE} 200ms` }}
        />
        <path
          d={line}
          pathLength={1}
          fill="none"
          stroke="currentColor"
          strokeWidth={1.5}
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeDasharray="1 1.1"
          style={{
            strokeDashoffset: drawn ? 0 : 1.05,
            transition: reduce || !drawn ? 'none' : `stroke-dashoffset 650ms ${EASE}`,
          }}
        />
        <circle
          cx={dotX}
          cy={dotY}
          r={2.75}
          fill="var(--background)"
          stroke="currentColor"
          strokeWidth={1.5}
          style={{ opacity: drawn ? 1 : 0, transition: reduce || !drawn ? 'none' : `opacity 200ms ${EASE} 550ms` }}
        />
      </svg>
      {active !== null ? (
        <div
          className="pointer-events-none absolute bottom-full z-10 mb-1.5 whitespace-nowrap rounded-md bg-foreground px-2 py-0.5 text-[12px] text-background tabular-nums"
          style={{ left: `${(dotX / W) * 100}%`, translate: `${(-index / (values.length - 1)) * 100}% 0` }}
        >
          <span className="text-background/60">v{labels[index]}</span> {format(values[index])}
        </div>
      ) : null}
    </div>
  )
}
