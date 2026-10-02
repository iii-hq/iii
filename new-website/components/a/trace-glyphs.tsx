'use client'

import { motion } from 'motion/react'
import {
  IconActivity,
  IconBot,
  IconDatabase,
  IconFlash,
  IconQueue,
  IconRocket2,
  IconTickSquare,
} from '@/components/icons/iconly'
import { cn } from '@/lib/utils'

/**
 * Glyphs for the hero trace, after the kind badges in the console's trace waterfall. All Iconly Bold (filled),
 * the one icon weight used across the site; `currentColor` so the parent decides the tone.
 */

type GlyphProps = { className?: string }

/** The traces mark in the panel header. */
export function TraceIcon({ className }: GlyphProps) {
  return <IconActivity className={cn('size-4', className)} />
}

const GLYPHS: Record<string, (p: GlyphProps) => React.JSX.Element> = {
  ai: IconBot,
  q: IconQueue,
  db: IconDatabase,
  ev: IconFlash,
  ci: IconTickSquare,
  sh: IconRocket2,
}

/** The glyph for a span kind (`ai`, `q`, `db`, `ev`, `ci`, `sh`), sized by the parent. */
export function KindGlyph({ kind, className }: { kind: string; className?: string }) {
  const Glyph = GLYPHS[kind] ?? IconBot
  return <Glyph className={cn('size-3', className)} />
}

type MiniSpan = { id: string; start: number; length: number; depth: number }

/**
 * The console's trace strip in miniature: every span of this trace as a hairline bar on a shared time axis, with a
 * playhead that sweeps the axis once while the trace runs. Pure SVG; the bars draw in on the same clock as the big
 * waterfall.
 */
export function TraceMinimap({
  spans,
  active,
  drawSeconds,
  className,
}: {
  spans: readonly MiniSpan[]
  active: boolean
  drawSeconds: number
  className?: string
}) {
  const W = 120
  const H = 26
  const X0 = 2
  const X1 = W - 2
  const scale = (X1 - X0) / 100
  const rowY = (i: number) => 3 + i * 3.4
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className={cn('h-[26px] w-[120px]', className)} aria-hidden>
      {/* axis and quarter ticks */}
      <path d={`M${X0} ${H - 1.5}H${X1}`} stroke="var(--line-strong)" strokeWidth={0.75} />
      {[0, 25, 50, 75, 100].map((p) => (
        <path key={p} d={`M${X0 + p * scale} ${H - 1.5}v-2`} stroke="var(--line-strong)" strokeWidth={0.75} />
      ))}
      {spans.map((span, i) => {
        const delay = (span.start / 100) * drawSeconds
        const duration = Math.max(0.2, (span.length / 100) * drawSeconds)
        return (
          <motion.rect
            key={span.id}
            x={X0 + span.start * scale}
            y={rowY(i)}
            width={Math.max(1.5, span.length * scale)}
            height={2}
            rx={1}
            fill={span.depth ? 'var(--foreground)' : 'var(--hero-accent)'}
            fillOpacity={span.depth ? 0.7 : 1}
            style={{ transformBox: 'fill-box', transformOrigin: 'left center' }}
            initial={active ? { scaleX: 0, opacity: 0 } : false}
            animate={{ scaleX: 1, opacity: 1 }}
            transition={{
              scaleX: { duration: active ? duration : 0, delay: active ? delay : 0, ease: 'linear' },
              opacity: { duration: 0.15, delay: active ? delay : 0 },
            }}
          />
        )
      })}
      {active ? (
        <motion.path
          d={`M${X0} 1V${H - 1.5}`}
          stroke="var(--foreground)"
          strokeWidth={0.75}
          strokeOpacity={0.6}
          initial={{ x: 0, opacity: 1 }}
          animate={{ x: X1 - X0, opacity: [1, 1, 0] }}
          transition={{
            x: { duration: drawSeconds, ease: 'linear' },
            opacity: { duration: drawSeconds + 0.3, times: [0, 0.97, 1] },
          }}
        />
      ) : null}
    </svg>
  )
}
