'use client'

import { motion } from 'motion/react'

import { joined, rate } from '@/lib/benchmarks/format'
import type { LocalRow } from '@/lib/benchmarks/views'
import { duration, easeOut } from '@/lib/motion'

import { useProfile } from './profile'

/**
 * Local dispatch on the selected machine size: in-process calls per second, unobserved and observed, as bars,
 * each with what the engine spent meanwhile. The point of the picture is the first bar's CPU: none.
 */
export function LocalBars({ rows }: { rows: LocalRow[] }) {
  const { selected, profiles } = useProfile()
  const exact = rows.find((r) => r.profile.name === selected)
  const row = exact ?? rows[0]
  if (!row) return null
  const chosen = profiles.find((p) => p.name === selected)
  const max = Math.max(...row.cases.map((c) => c.rate), 1)
  return (
    <>
      <p className="px-3 pb-3 text-[12.5px] text-muted-foreground">
        {exact ? row.profile.label : `${chosen?.label ?? selected} did not run this; showing ${row.profile.label}`}
      </p>
      <ul className="space-y-5 px-3 pt-1 pb-3 sm:px-3">
        {row.cases.map((c, i) => (
          <li key={c.label}>
            <div className="flex items-baseline justify-between gap-4 text-[13px]">
              <span className="text-foreground">{c.label.charAt(0).toUpperCase() + c.label.slice(1)}</span>
              <span className="font-mono text-foreground tabular-nums">{joined(rate(c.rate))}</span>
            </div>
            <div className="mt-2 h-2 overflow-hidden rounded-full bg-muted">
              <motion.div
                className="h-full origin-left rounded-full bg-foreground"
                initial={{ scaleX: 0 }}
                whileInView={{ scaleX: c.rate / max }}
                animate={{ scaleX: c.rate / max }}
                viewport={{ once: true, margin: '0px 0px -10% 0px' }}
                transition={{ duration: duration.reveal * 1.6, delay: i * 0.08, ease: easeOut }}
              />
            </div>
            <p className="mt-1.5 text-[12.5px] text-muted-foreground tabular-nums">
              engine {c.engineCpu.toFixed(2)} CPU
              {c.activations ? ` · ${joined(rate(c.activations))} activations delivered` : ' · nothing on the wire'}
            </p>
          </li>
        ))}
      </ul>
    </>
  )
}
