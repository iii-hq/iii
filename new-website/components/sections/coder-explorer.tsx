'use client'

import { motion } from 'motion/react'
import { useEffect, useId, useState } from 'react'
import { CoderCompose } from '@/components/graphics/coder-compose'
import { CoderDiscover } from '@/components/graphics/coder-discover'
import { CoderExtend } from '@/components/graphics/coder-extend'
import { CoderObserve } from '@/components/graphics/coder-observe'
import { CoderReact } from '@/components/graphics/coder-react'
import { DemoPlayback } from '@/components/graphics/demo-playback'
import { GraphicPlaybackContext } from '@/components/graphics/graphic-playback-context'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { spring } from '@/lib/motion'
import { cn } from '@/lib/utils'

export type CoderId = 'composability' | 'observability' | 'discoverability' | 'extensibility' | 'reactivity'
export type CoderItem = {
  id: CoderId
  letter: string
  name: string
  short: string
  hint: string
  claim: string
  body: string
  example: string
}

const GRAPHICS = {
  composability: CoderCompose,
  observability: CoderObserve,
  discoverability: CoderDiscover,
  extensibility: CoderExtend,
  reactivity: CoderReact,
}

export function CoderExplorer({ items }: { items: CoderItem[] }) {
  const id = useId()
  const { ref, reduce, paused, setPaused, running } = useDemoPlayback()
  const [index, setIndex] = useState(0)
  const [pinned, setPinned] = useState(false)
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const current = items[index]
  const Graphic = GRAPHICS[current.id]

  useEffect(() => {
    if (!running || pinned || hovered || focused) return
    const timer = window.setInterval(() => setIndex((value) => (value + 1) % items.length), 12000)
    return () => window.clearInterval(timer)
  }, [running, pinned, hovered, focused, items.length])

  return (
    <div ref={ref} data-running={running} data-coder-explorer className="mt-12 md:mt-14">
      <div className="grid min-w-0 gap-8 lg:grid-cols-[minmax(0,0.7fr)_minmax(0,1.5fr)] lg:gap-16">
        <div
          onPointerEnter={(event) => event.pointerType === 'mouse' && setHovered(true)}
          onPointerLeave={() => setHovered(false)}
          onFocusCapture={() => setFocused(true)}
          onBlurCapture={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false)
          }}
        >
          <div className="grid grid-cols-5 gap-1 border-b pb-3 lg:grid-cols-1 lg:gap-0 lg:border-b-0 lg:pb-0">
            {items.map((item, itemIndex) => (
              <button
                key={item.id}
                type="button"
                aria-label={item.name}
                aria-pressed={itemIndex === index}
                aria-controls={`${id}-property`}
                onClick={() => {
                  setIndex(itemIndex)
                  setPinned(true)
                }}
                className={cn(
                  'group relative flex min-h-16 flex-col items-center gap-2 rounded-md px-1 py-3 text-left outline-none transition-colors focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2 lg:min-h-[88px] lg:flex-row lg:gap-4 lg:rounded-none lg:border-b lg:px-4',
                  itemIndex === index ? 'bg-faint text-foreground' : 'text-muted-foreground hover:text-foreground',
                )}
              >
                <span
                  aria-hidden
                  className={cn(
                    'flex size-8 shrink-0 items-center justify-center rounded-md border font-mono text-lg transition-colors',
                    itemIndex === index ? 'border-line-strong text-foreground' : 'border-transparent',
                  )}
                >
                  {item.letter}
                </span>
                <span className="text-[11px] sm:text-xs lg:hidden">{item.short}</span>
                <span className="hidden min-w-0 lg:block">
                  <span className="block font-medium text-[15px]">{item.name}</span>
                  <span className="mt-1 block text-[12px] text-muted-foreground">{item.hint}</span>
                </span>
                {itemIndex === index ? (
                  <motion.span
                    layoutId="coder-rail-indicator"
                    aria-hidden
                    className="absolute inset-x-2 -bottom-3 h-px bg-foreground lg:inset-x-auto lg:inset-y-4 lg:left-0 lg:h-auto lg:w-px"
                    transition={spring.snappy}
                  />
                ) : null}
              </button>
            ))}
          </div>
        </div>
        <section id={`${id}-property`} aria-label={current.name} className="graphic-stage min-w-0">
          <div className="flex items-center justify-between gap-3 border-b pb-3">
            <p className="font-mono text-[12px] text-foreground uppercase tracking-[0.08em]">{current.name}</p>
            <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
          </div>
          <div className="mt-6 grid min-h-[170px] pb-6 sm:min-h-[135px]">
            {items.map((item) => (
              <div key={item.id} aria-hidden className="invisible [grid-area:1/1]">
                <PropertyCopy item={item} />
              </div>
            ))}
            <div key={current.id} className="swap-in [grid-area:1/1]">
              <PropertyCopy item={current} />
            </div>
          </div>
          <GraphicPlaybackContext value={running}>
            <div key={current.id} className="swap-in relative mx-auto min-w-0 max-w-[600px] py-2">
              <Graphic />
            </div>
          </GraphicPlaybackContext>
          <p
            key={current.id}
            className="swap-in mt-8 min-h-20 border-t border-dashed pt-4 font-mono text-[12px] text-muted-foreground leading-relaxed sm:min-h-12"
          >
            {current.example}
          </p>
        </section>
      </div>
    </div>
  )
}

function PropertyCopy({ item }: { item: CoderItem }) {
  return (
    <>
      <h3 className="text-balance font-medium text-2xl tracking-tight">{item.claim}</h3>
      <p className="mt-3 max-w-xl text-pretty text-[14px] text-muted-foreground leading-relaxed">{item.body}</p>
    </>
  )
}
