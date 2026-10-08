'use client'

import { ArrowUpRightIcon } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { type CSSProperties, useEffect, useState } from 'react'
import { IconTickSquare } from '@/components/icons/iconly'
import { useMotionPreference } from '@/hooks/use-motion-preference'
import type { CodeLine } from '@/lib/highlight'
import { easeInOut } from '@/lib/motion'
import { cn } from '@/lib/utils'
import { HeroAsciiCore } from './hero-ascii-core'
import { heroScenes as scenes } from './hero-scenes'
import { useGraphicLoop } from './use-graphic-loop'

const workers = [
  {
    id: 'node',
    name: 'Node',
    fn: 'reports::generate',
    path: 'M524 200H539Q551 200 551 188V98Q551 86 563 86H586',
    incoming: 'M586 86H563Q551 86 551 98V188Q551 200 539 200H524',
  },
  {
    id: 'python',
    name: 'Python',
    fn: 'iii.trigger()',
    path: 'M524 200H551Q563 200 563 188V172Q563 160 575 160H586',
    incoming: 'M586 160H575Q563 160 563 172V188Q563 200 551 200H524',
  },
  {
    id: 'queue',
    name: 'Queue',
    fn: 'reports',
    path: 'M524 200H551Q563 200 563 212V222Q563 234 575 234H586',
    incoming: 'M586 234H575Q563 234 563 222V212Q563 200 551 200H524',
  },
  {
    id: 'agent',
    name: 'AI agent',
    fn: 'harness::send',
    path: 'M524 200H539Q551 200 551 212V296Q551 308 563 308H586',
    incoming: 'M586 308H563Q551 308 551 296V212Q551 200 539 200H524',
  },
] as const

/** A scripted example: register a function, call it across languages, then trace a queued call. */
export function HeroRuntime({ code, className }: { code: CodeLine[][]; className?: string }) {
  const { ref, inView } = useGraphicLoop<HTMLDivElement>()
  const reduce = useMotionPreference()
  const [tick, setTick] = useState(0)
  const [pageVisible, setPageVisible] = useState(true)
  const running = inView && !reduce && pageVisible
  const sceneIndex = tick % scenes.length
  const scene = scenes[sceneIndex]

  useEffect(() => {
    const onVisibility = () => setPageVisible(!document.hidden)
    onVisibility()
    document.addEventListener('visibilitychange', onVisibility)
    return () => document.removeEventListener('visibilitychange', onVisibility)
  }, [])

  // biome-ignore lint/correctness/useExhaustiveDependencies: A new step or replay must restart the full scene timer.
  useEffect(() => {
    if (!running) return
    const timer = window.setTimeout(() => setTick((value) => value + 1), 4200)
    return () => window.clearTimeout(timer)
  }, [running, tick])

  return (
    <figure ref={ref} className={cn('relative min-w-0 @container', className)}>
      <figcaption className="sr-only">
        A function is registered with the iii engine. Python and queue workers call it by name over WebSocket, and the
        engine routes each call to its Node worker in one trace.
      </figcaption>
      <div className="flex items-center justify-between gap-3 border-b border-dashed pb-3 font-sans text-[12px] text-muted-foreground @min-[680px]:text-[13px]">
        <span className="flex items-center gap-2">
          <span aria-hidden className="size-1.5 rounded-full bg-ok" />
          Connected to iii
        </span>
        <span>JSON over WebSocket</span>
      </div>
      <div className="relative grid grid-cols-1 items-center gap-y-5 py-5 @min-[680px]:min-h-[420px] @min-[680px]:grid-cols-[1.1fr_1fr_0.85fr] @min-[680px]:gap-x-4 @min-[680px]:gap-y-6 @min-[680px]:py-8 @min-[1000px]:min-h-[480px] @min-[1000px]:gap-x-8 @min-[1000px]:py-10">
        <div
          aria-hidden
          className="pointer-events-none absolute -inset-y-10 inset-x-0 bg-dots opacity-50 [mask-image:radial-gradient(ellipse_at_center,black,transparent_75%)]"
        />
        <svg
          viewBox="0 0 800 400"
          preserveAspectRatio="none"
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 hidden h-full w-full @min-[680px]:block"
        >
          <path d="M280 200H358" stroke="var(--line-strong)" strokeDasharray="3 5" fill="none" />
          {workers.map((worker) => (
            <path key={worker.id} d={worker.path} stroke="var(--line-strong)" fill="none" />
          ))}
          <AnimatePresence initial={false}>
            {running ? (
              <motion.g key={tick} exit={{ opacity: 0 }} transition={{ duration: 0.2 }}>
                <motion.path
                  d={workers[sceneIndex].incoming}
                  fill="none"
                  stroke="var(--hero-accent)"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  initial={{ pathLength: 0, opacity: 0 }}
                  animate={{ pathLength: [0, 1, 1], opacity: [0, 1, 0] }}
                  transition={{ duration: 1.6, delay: 0.55, times: [0, 0.75, 1], ease: easeInOut }}
                />
                <motion.path
                  d={workers[0].path}
                  fill="none"
                  stroke="var(--hero-accent)"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  initial={{ pathLength: 0, opacity: 0 }}
                  animate={{ pathLength: [0, 1, 1], opacity: [0, 1, 0] }}
                  transition={{ duration: 2, delay: 1.6, times: [0, 0.8, 1], ease: easeInOut }}
                />
                <motion.circle
                  r="3"
                  cy="200"
                  fill="var(--hero-accent)"
                  initial={{ cx: 280, opacity: 0 }}
                  animate={{ cx: [280, 358], opacity: [0, 1, 1, 0] }}
                  transition={{ duration: 1.1, delay: 0.4, ease: easeInOut }}
                />
              </motion.g>
            ) : null}
          </AnimatePresence>
          <circle cx="358" cy="200" r="2.5" fill="var(--background)" stroke="var(--line-strong)" />
          <circle cx="524" cy="200" r="2.5" fill="var(--background)" stroke="var(--hero-accent)" />
        </svg>
        <div className="relative min-w-0 overflow-hidden rounded-xl border bg-card/70 shadow-[0_8px_30px_-12px_var(--line)]">
          <div className="flex items-center justify-between gap-3 border-b px-3 py-3 @min-[850px]:px-4">
            <span className="font-mono text-[13px]">{scene.file}</span>
            <span className="font-sans text-xs text-muted-foreground">{scene.language}</span>
          </div>
          <div className="min-h-[240px] overflow-x-auto px-3 py-4 @min-[680px]:min-h-[244px] @min-[850px]:px-4 [scrollbar-width:thin]">
            <AnimatePresence initial={false} mode="wait">
              <motion.pre
                key={tick}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
                className="font-mono text-[13px] leading-[1.6] @min-[1000px]:text-sm"
              >
                <code className="code-tokens">
                  {code[sceneIndex].map((line, index) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: Lines are static per scene and never reorder.
                    <span key={index} className="flex">
                      <span
                        aria-hidden
                        className="hidden w-6 shrink-0 select-none text-muted-foreground @min-[850px]:block"
                      >
                        {index + 1}
                      </span>
                      <motion.span
                        className="block min-w-0 whitespace-pre text-muted-foreground"
                        initial={running ? { clipPath: 'inset(0 100% 0 0)' } : false}
                        animate={{ clipPath: 'inset(0 0% 0 0)' }}
                        transition={{ duration: running ? 0.4 : 0, delay: running ? index * 0.16 : 0, ease: 'linear' }}
                      >
                        {line.map((token, position) => (
                          // biome-ignore lint/suspicious/noArrayIndexKey: Tokens are static per line and never reorder.
                          <span key={position} style={token.style as CSSProperties}>
                            {token.text}
                          </span>
                        ))}
                        {'\n'}
                      </motion.span>
                    </span>
                  ))}
                </code>
              </motion.pre>
            </AnimatePresence>
          </div>
          <div className="flex items-center gap-2 border-t bg-faint px-3 py-2.5 font-mono text-xs text-muted-foreground">
            <ArrowUpRightIcon aria-hidden className="size-3.5" />
            reports::generate
          </div>
        </div>
        <div className="relative mx-auto w-full max-w-[250px] pb-6 @min-[680px]:max-w-[340px] @min-[1000px]:max-w-[400px]">
          <HeroAsciiCore running={running} sequence={tick} />
          <p className="absolute inset-x-0 bottom-1 text-center font-mono text-xs text-muted-foreground">
            [ iii engine ]
          </p>
        </div>
        <div className="relative grid min-w-0 grid-cols-2 gap-2.5 @min-[680px]:flex @min-[680px]:flex-col @min-[680px]:gap-3">
          {workers.map((worker) => (
            <div
              key={worker.id}
              style={{ borderColor: worker.id === scene.worker ? 'var(--hero-accent)' : 'var(--line)' }}
              className="flex min-w-0 items-center gap-2 rounded-lg border bg-background/90 px-3 py-2.5 transition-[border-color] duration-300 motion-reduce:transition-none @min-[680px]:py-3 @min-[850px]:px-3.5"
            >
              <div className="min-w-0 flex-1">
                <p className="font-sans text-sm leading-[21px]">{worker.name}</p>
                <p className="font-mono text-xs text-muted-foreground">
                  {worker.fn.includes('::') ? (
                    <>
                      {worker.fn.split('::')[0]}::
                      <wbr />
                      {worker.fn.split('::')[1]}
                    </>
                  ) : (
                    worker.fn
                  )}
                </p>
              </div>
              {worker.id === scene.worker ? <IconTickSquare className="size-3.5 shrink-0 text-hero-accent" /> : null}
            </div>
          ))}
        </div>
      </div>
    </figure>
  )
}
