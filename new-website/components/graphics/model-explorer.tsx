'use client'

import { ArrowRightIcon } from 'lucide-react'
import { motion } from 'motion/react'
import { type KeyboardEvent, useEffect, useId, useRef, useState } from 'react'

import { CopyButton } from '@/components/animate-ui/components/buttons/copy'
import { IconCode, IconFlash, IconPackage, IconTickSquare, IconTimeCircle } from '@/components/icons/iconly'
import { Logo } from '@/components/site/logo'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { spring } from '@/lib/motion'
import { cn } from '@/lib/utils'
import { DemoPlayback } from './demo-playback'
import { completeExample, type ExamplePhase, type Primitive, primitives } from './model-example'
import styles from './model-explorer.module.css'

const icons = { worker: IconPackage, function: IconCode, trigger: IconFlash }
const focusRing = 'focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-4'

const phaseLabels: Record<ExamplePhase, string> = {
  idle: 'Follow a scheduled call through the engine.',
  routing: 'Schedule fired. The engine finds reports::daily.',
  executing: 'The reports worker runs the function.',
  complete: 'Completed. Result: { "status": "ready" }',
}

/** A repeating call links the three definitions, code, and architecture. */
export function ModelExplorer({ code }: { code: Record<Primitive, string> }) {
  const id = useId()
  const { ref, reduce, paused, setPaused, running } = useDemoPlayback()
  const [selected, setSelected] = useState<Primitive>('worker')
  const [step, setStep] = useState<ExamplePhase>('idle')
  const phase = reduce ? 'complete' : step
  const [copied, setCopied] = useState(false)
  const tabs = useRef<Partial<Record<Primitive, HTMLButtonElement | null>>>({})
  const current = primitives.find((primitive) => primitive.id === selected) ?? primitives[0]

  useEffect(() => {
    if (!running) return
    const pending = new Set<number>()
    const schedule = (callback: () => void, delay: number) => {
      const timer = window.setTimeout(() => {
        pending.delete(timer)
        callback()
      }, delay)
      pending.add(timer)
    }
    const cycle = () => {
      setStep('routing')
      schedule(() => setStep('executing'), 850)
      schedule(() => setStep('complete'), 1700)
      schedule(cycle, 5000)
    }
    schedule(cycle, 300)
    return () => pending.forEach(window.clearTimeout)
  }, [running])

  const select = (next: Primitive) => {
    setSelected(next)
  }

  const navigate = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let next: number
    if (event.key === 'ArrowRight') next = (index + 1) % primitives.length
    else if (event.key === 'ArrowLeft') next = (index + primitives.length - 1) % primitives.length
    else if (event.key === 'Home') next = 0
    else if (event.key === 'End') next = primitives.length - 1
    else return
    event.preventDefault()
    const target = primitives[next].id
    select(target)
    tabs.current[target]?.focus()
  }

  return (
    <div className="mt-12 md:mt-14">
      <div role="tablist" aria-label="Explore the three primitives" className="grid grid-cols-3 border-b">
        {primitives.map((primitive, index) => {
          const Icon = icons[primitive.id]
          const active = selected === primitive.id
          return (
            <button
              key={primitive.id}
              ref={(element) => {
                tabs.current[primitive.id] = element
              }}
              type="button"
              role="tab"
              id={`${id}-${primitive.id}`}
              aria-controls={`${id}-panel`}
              aria-selected={active}
              tabIndex={active ? 0 : -1}
              onClick={() => select(primitive.id)}
              onKeyDown={(event) => navigate(event, index)}
              className={cn(
                styles.tab,
                focusRing,
                'relative min-w-0 py-5 text-left sm:px-5 first:sm:pl-0 last:sm:pr-0',
                active ? 'text-foreground' : 'text-muted-foreground',
              )}
            >
              <span className="flex items-center gap-2.5 sm:gap-3">
                <Icon className="hidden size-[18px] shrink-0 sm:block" />
                <span className="font-medium text-[15px] tracking-tight sm:text-xl">{primitive.name}</span>
              </span>
              <span className="mt-2 hidden text-pretty text-[14px] text-muted-foreground leading-relaxed sm:block">
                {primitive.summary}
              </span>
              {active ? (
                <motion.span
                  layoutId="model-tab-indicator"
                  aria-hidden
                  className="absolute inset-x-0 -bottom-px h-px bg-foreground"
                  transition={spring.snappy}
                />
              ) : null}
            </button>
          )
        })}
      </div>

      <div
        role="tabpanel"
        id={`${id}-panel`}
        aria-labelledby={`${id}-${selected}`}
        // biome-ignore lint/a11y/noNoninteractiveTabindex: A tabpanel is focusable so keyboard users can reach its explanatory content.
        tabIndex={0}
        className={cn('mt-8 min-w-0 outline-offset-8 md:mt-10', focusRing)}
      >
        <div className="grid min-w-0 gap-6 lg:grid-cols-[minmax(0,0.85fr)_minmax(0,1.4fr)] lg:grid-rows-[auto_1fr] lg:gap-x-14 lg:gap-y-0">
          <div key={selected} className="swap-in min-h-[116px] min-w-0 sm:min-h-[100px] lg:col-start-1 lg:row-start-1">
            <h3 className="text-balance font-medium text-lg tracking-tight">{current.heading}</h3>
            <p className="mt-2 max-w-md text-pretty text-[14px] text-muted-foreground leading-relaxed">
              {current.description}
            </p>
          </div>
          <div
            ref={ref}
            data-running={running}
            className={cn(styles.playback, 'graphic-stage min-w-0 lg:col-start-2 lg:row-span-2 lg:row-start-1')}
          >
            <div className="flex items-center justify-between gap-3 border-b border-dashed pb-3">
              <div>
                <p className="font-medium text-sm">A daily report, end to end.</p>
                <p className="mt-1 text-xs text-muted-foreground">A scheduled call through iii</p>
              </div>
              <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
            </div>
            <Architecture selected={selected} phase={phase} />
            <output
              aria-live="off"
              aria-atomic="true"
              className="flex min-h-[60px] items-center gap-2.5 border-t border-dashed py-3 text-[12px] text-muted-foreground sm:font-mono"
            >
              {phase === 'complete' ? (
                <IconTickSquare className="size-4 shrink-0 text-foreground" />
              ) : (
                <ArrowRightIcon aria-hidden className="size-4 shrink-0" />
              )}
              <span>{phaseLabels[phase]}</span>
            </output>
          </div>

          <div className="min-w-0 lg:col-start-1 lg:row-start-2">
            <div className="overflow-hidden rounded-xl border bg-card">
              <div className="flex h-11 items-center justify-between gap-3 border-b pr-1 pl-4">
                <span className="font-mono text-xs text-muted-foreground">
                  reports.ts <span className="ml-2 text-foreground/40">/ {current.name.toLowerCase()}</span>
                </span>
                <CopyButton
                  content={completeExample}
                  variant="ghost"
                  size="default"
                  aria-label="Copy complete example"
                  onCopiedChange={setCopied}
                  className="text-muted-foreground hover:text-foreground"
                />
              </div>
              <div
                key={selected}
                className="swap-in code-numbered min-h-[215px] overflow-x-auto px-4 py-4 font-mono text-[13px] leading-[1.7] [scrollbar-width:thin]"
                // biome-ignore lint/security/noDangerouslySetInnerHtml: Shiki output is generated on the server from static strings.
                dangerouslySetInnerHTML={{ __html: code[selected] }}
              />
            </div>
            <output aria-live="polite" className="mt-2 block min-h-5 text-xs text-muted-foreground">
              {copied ? 'Copied the complete example.' : ''}
            </output>
          </div>
        </div>
        <p className="mt-7 max-w-2xl text-pretty text-[14px] text-muted-foreground leading-relaxed">
          <span className="text-foreground">One engine connects all three.</span> It keeps the live registry and routes
          each call to the right worker.
        </p>
      </div>
    </div>
  )
}

function Architecture({ selected, phase }: { selected: Primitive; phase: ExamplePhase }) {
  const executing = phase === 'executing'
  return (
    <div className={cn(styles.canvas, 'relative py-8 sm:py-10')}>
      <div className={styles.diagram} data-phase={phase}>
        <div className={cn(styles.node, styles.trigger)} data-highlight={selected === 'trigger' || phase === 'routing'}>
          <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.06em] text-muted-foreground">
            <IconTimeCircle className="size-3.5" />
            Trigger
          </div>
          <p className="mt-3 font-medium text-[14px]">Daily schedule</p>
          <p className="mt-1 font-mono text-[12px] text-muted-foreground">0 0 9 * * * *</p>
        </div>
        <Bridge outgoing={false} />
        <div className={styles.engine}>
          <div className={styles.chip}>
            <Logo className="h-8 text-foreground" />
          </div>
          <span className="absolute top-full mt-3 whitespace-nowrap font-mono text-[11px] text-muted-foreground">
            iii engine
          </span>
        </div>
        <Bridge outgoing />
        <div className={cn(styles.node, styles.worker)} data-highlight={selected === 'worker' || executing}>
          <div className="flex items-center justify-between gap-2 border-b px-4 py-3">
            <span className="flex items-center gap-2 font-mono text-[11px] text-muted-foreground uppercase tracking-[0.06em]">
              <IconPackage className="size-3.5" />
              Worker
            </span>
            <span className="font-mono text-[11px] text-muted-foreground">Node.js</span>
          </div>
          <div className="p-4">
            <p className="mb-3 font-mono text-[12px] text-muted-foreground">reports</p>
            <div className={cn(styles.node, styles.function)} data-highlight={selected === 'function' || executing}>
              <span className="flex items-center gap-2 font-mono text-[11px] text-muted-foreground uppercase tracking-[0.06em]">
                <IconCode className="size-3.5" />
                Function
              </span>
              <p className="mt-2 whitespace-nowrap font-mono text-[13px] text-foreground">reports::daily</p>
              <div aria-hidden className={styles.progress} />
            </div>
          </div>
        </div>
      </div>
      <p className="relative mt-6 text-center font-mono text-[11px] text-muted-foreground">JSON over WebSocket</p>
    </div>
  )
}

/** Drawn at 1:1 CSS pixels (48 × 24) so the hairline and dots stay crisp instead of scaling with the viewBox. */
function Bridge({ outgoing }: { outgoing: boolean }) {
  return (
    <svg
      viewBox="0 0 48 24"
      width="48"
      height="24"
      aria-hidden="true"
      className={cn(styles.bridge, outgoing ? styles.outgoing : styles.incoming)}
    >
      <path
        d="M1 12.5H47"
        fill="none"
        stroke="var(--line-strong)"
        strokeLinecap="round"
        strokeDasharray="0 5"
        vectorEffect="non-scaling-stroke"
      />
      <circle cx="4" cy="12.5" r="2.5" fill="var(--hero-accent)" className={styles.packet} />
    </svg>
  )
}
