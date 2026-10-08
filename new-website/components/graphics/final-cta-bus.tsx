'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'

import { cn } from '@/lib/utils'
import { useGraphicLoop } from './use-graphic-loop'

type Joiner = { label: string; fn: string; x: number; side: -1 | 1 }

const JOINERS: Joiner[] = [
  { label: 'your-service.ts', fn: 'orders::create', x: 150, side: -1 },
  { label: 'billing.py', fn: 'billing::charge', x: 320, side: 1 },
  { label: 'claude-code', fn: 'agent::turn', x: 490, side: -1 },
  { label: 'resize.rs', fn: 'images::resize', x: 660, side: 1 },
  { label: 'browser tab', fn: 'ui::notify', x: 830, side: -1 },
]

const BUS_Y = 130
const TRACE_Y = 262
const ease = [0.22, 1, 0.36, 1] as const

/**
 * Final CTA graphic: one worker connects first, then every worker added after it
 * joins the same engine registry and appends its span to the same trace.
 */
export function FinalCtaBus({ className }: { className?: string }) {
  const { ref, active, inView } = useGraphicLoop<HTMLDivElement>()
  // count = number of joined workers; the loop holds on the full system, then resets.
  const [count, setCount] = useState(JOINERS.length)

  useEffect(() => {
    if (!active) {
      setCount(JOINERS.length)
      return
    }
    setCount(1)
    let n = 1
    const id = window.setInterval(() => {
      n = n >= JOINERS.length + 2 ? 1 : n + 1
      setCount(Math.min(n, JOINERS.length))
    }, 1300)
    return () => window.clearInterval(id)
  }, [active])

  const shown = inView || !active ? count : JOINERS.length

  return (
    <div ref={ref} className={cn('relative', className)}>
      <svg
        viewBox="0 0 980 310"
        role="img"
        aria-label="One worker connects to the engine first; every worker added after it joins the same registry and adds its span to the same trace."
        className="h-auto w-full"
      >
        {/* Engine bus */}
        <rect x={40} y={BUS_Y - 16} width={900} height={32} rx={16} fill="var(--node)" stroke="var(--line-strong)" />
        <text x={62} y={BUS_Y + 4} className="fill-muted-foreground font-mono" fontSize={10} letterSpacing="0.12em">
          ENGINE
        </text>
        <text
          x={918}
          y={BUS_Y + 4}
          textAnchor="end"
          className="fill-muted-foreground font-mono"
          fontSize={10}
          letterSpacing="0.04em"
        >
          registry · {shown} {shown === 1 ? 'worker' : 'workers'}
        </text>

        {/* Trace */}
        <text x={40} y={TRACE_Y - 22} className="fill-muted-foreground font-mono" fontSize={10} letterSpacing="0.12em">
          ONE TRACE
        </text>
        <line x1={40} y1={TRACE_Y} x2={940} y2={TRACE_Y} stroke="var(--line)" strokeDasharray="2 5" />

        <AnimatePresence initial={false}>
          {JOINERS.slice(0, shown).map((j, i) => {
            const nodeY = j.side === -1 ? 58 : 196
            const linkFrom = j.side === -1 ? nodeY + 18 : nodeY - 18
            const linkTo = j.side === -1 ? BUS_Y - 16 : BUS_Y + 16
            const spanX = 40 + i * 150
            return (
              <motion.g
                key={j.label}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0, transition: { duration: 0.3 } }}
                transition={{ duration: 0.4 }}
              >
                <motion.line
                  x1={j.x}
                  y1={linkFrom}
                  x2={j.x}
                  y2={linkTo}
                  stroke="var(--hero-accent)"
                  strokeWidth={1.25}
                  initial={{ pathLength: 0 }}
                  animate={{ pathLength: 1 }}
                  transition={{ duration: 0.5, delay: 0.25, ease }}
                />
                <motion.circle
                  cx={j.x}
                  cy={linkTo}
                  r={3}
                  fill="var(--foreground)"
                  initial={{ scale: 0 }}
                  animate={{ scale: 1 }}
                  transition={{ duration: 0.3, delay: 0.7, ease }}
                  style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
                />
                <motion.g initial={{ y: j.side * -10 }} animate={{ y: 0 }} transition={{ duration: 0.5, ease }}>
                  <rect
                    x={j.x - 70}
                    y={nodeY - 18}
                    width={140}
                    height={36}
                    rx={10}
                    fill="var(--node)"
                    stroke={i === 0 ? 'var(--hero-accent)' : 'var(--line-strong)'}
                  />
                  <text x={j.x} y={nodeY + 4} textAnchor="middle" className="fill-foreground font-mono" fontSize={11}>
                    {j.label}
                  </text>
                </motion.g>
                <motion.rect
                  x={spanX}
                  y={TRACE_Y - 6}
                  height={12}
                  rx={3}
                  fill={i === 0 ? 'var(--foreground)' : 'var(--line-strong)'}
                  initial={{ width: 0 }}
                  animate={{ width: 140 }}
                  transition={{ duration: 0.6, delay: 0.7, ease }}
                />
                <motion.text
                  x={spanX + 2}
                  y={TRACE_Y + 26}
                  className="fill-muted-foreground font-mono"
                  fontSize={10}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={{ duration: 0.4, delay: 0.9 }}
                >
                  {j.fn}
                </motion.text>
              </motion.g>
            )
          })}
        </AnimatePresence>
      </svg>
    </div>
  )
}
