'use client'

import { motion } from 'motion/react'
import { useEffect, useState } from 'react'

import { useGraphicLoop } from '@/components/graphics/use-graphic-loop'
import { spring } from '@/lib/motion'
import { cn } from '@/lib/utils'

import { ownership } from './content'
import { SystemGraph } from './graph/system-graph'

/** The engine, graph intact, picks itself up from one environment and drops into the next. */
export function OwnershipMove() {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const [slot, setSlot] = useState(0)
  useEffect(() => {
    if (!active) return
    const id = window.setInterval(() => setSlot((s) => (s + 1) % ownership.environments.length), 2600)
    return () => window.clearInterval(id)
  }, [active])

  return (
    <div ref={ref} className="graphic-stage">
      <div className="relative grid grid-cols-3 gap-3">
        {ownership.environments.map((env, i) => (
          <div
            key={env}
            className={cn(
              'flex aspect-[4/3] flex-col rounded-xl border border-dashed p-3 transition-colors duration-500 sm:aspect-[5/4]',
              i === slot ? 'border-line-strong' : 'border-border',
            )}
          >
            <p
              className={cn(
                'mt-auto font-sans text-[13px] uppercase tracking-[0.08em]',
                i === slot ? 'text-foreground' : 'text-muted-foreground',
              )}
            >
              {env}
            </p>
          </div>
        ))}
        <motion.div
          aria-hidden
          className="pointer-events-none absolute inset-y-0 w-[calc((100%-1.5rem)/3)] p-2 sm:p-3"
          initial={false}
          animate={{ left: `calc(${slot} * ((100% - 1.5rem) / 3 + 0.75rem))`, y: [0, -10, 0] }}
          transition={{ left: spring.soft, y: { duration: 0.9, times: [0, 0.4, 1] } }}
        >
          <div className="h-[calc(100%-1.75rem)] rounded-lg border bg-card p-2 shadow-[0_12px_40px_-16px_rgb(0_0_0/0.6)]">
            <SystemGraph
              stage="react"
              step={5}
              active={false}
              label="The complete iii graph, carried unchanged between Local, Self-hosted and Cloud."
              className="h-full w-full"
            />
          </div>
        </motion.div>
      </div>
    </div>
  )
}
