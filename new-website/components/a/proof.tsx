'use client'

import { ArrowRightIcon } from 'lucide-react'
import { motion } from 'motion/react'

import { useGraphicLoop } from '@/components/graphics/use-graphic-loop'
import { IconCheckCircle } from '@/components/icons/iconly'
import { Reveal } from '@/components/site/reveal'
import { buttonVariants } from '@/components/ui/button'
import { easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'
import { demo, proof } from './content'
import { HOPS } from './graph/model'
import { GraphScroller } from './graph/scroller'
import { SystemGraph } from './graph/system-graph'
import { useStageClock } from './graph/use-stage-clock'
import { Section } from './section'

/** Harness + ADE: the ADE session on the left issues the calls; the graph on the right lights up as they run. */
export function Proof() {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const { step, cycle } = useStageClock('harness', active)
  const done = step >= HOPS.length

  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section id="proof" eyebrow={proof.eyebrow} title={proof.title} lede={proof.subtitle}>
      <div
        ref={ref}
        className="mt-10 grid grid-cols-[minmax(0,1fr)] gap-8 lg:mt-14 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-14"
      >
        <Reveal className="flex min-w-0 flex-col gap-6">
          <div className="overflow-hidden rounded-xl border bg-card shadow-[0_8px_30px_-12px_var(--line)]">
            <div className="flex h-10 items-center gap-2 border-b px-3.5">
              <span aria-hidden className="flex gap-1.5">
                <span className="size-2.5 rounded-full bg-muted-foreground/30" />
                <span className="size-2.5 rounded-full bg-muted-foreground/30" />
                <span className="size-2.5 rounded-full bg-muted-foreground/30" />
              </span>
              <span className="ml-2 font-sans text-[13px] text-muted-foreground">ADE · harness session 7f3a</span>
            </div>
            <div className="px-3.5 pt-3.5">
              <p className="rounded-lg bg-faint px-3 py-2 font-sans text-[13px] text-foreground">› {demo.prompt}</p>
            </div>
            <ol className="flex flex-col gap-0.5 px-3.5 py-3">
              {HOPS.map((hop, i) => {
                const shown = !active || step >= i
                const now = active && step === i
                return (
                  <motion.li
                    key={hop.fn}
                    initial={false}
                    animate={{ opacity: shown ? 1 : 0.25, x: shown ? 0 : -4 }}
                    transition={{ duration: 0.3, ease: easeOut }}
                    className={cn(
                      'flex items-center gap-2.5 rounded-md px-2 py-1.5 font-sans text-[13px] transition-colors duration-300',
                      now ? 'bg-faint text-foreground' : 'text-muted-foreground',
                    )}
                  >
                    {shown && !now ? (
                      <IconCheckCircle className="size-4 shrink-0 text-hero-accent" />
                    ) : (
                      <span
                        aria-hidden
                        className={cn(
                          'size-4 shrink-0 rounded-full border',
                          now ? 'border-foreground' : 'border-border',
                        )}
                      />
                    )}
                    <span className="truncate font-mono text-foreground">{hop.fn}</span>
                    <span className="ml-auto shrink-0 tabular-nums">{hop.ms.toLocaleString('en-US')}ms</span>
                  </motion.li>
                )
              })}
            </ol>
            <div className="border-t border-dashed px-3.5 py-3 font-sans text-[13px] text-muted-foreground">
              {done || !active ? (
                <span className="text-foreground">digest posted to #releases · 3 PRs · watching #418</span>
              ) : (
                <span>
                  running step {Math.min(step + 1, HOPS.length)} of {HOPS.length}…
                </span>
              )}
            </div>
          </div>
          <p className="max-w-[560px] text-pretty text-[15px] text-muted-foreground leading-relaxed">
            {proof.solution}
          </p>
          <a
            href={proof.cta.href}
            className={cn(buttonVariants(), 'group h-11 w-fit gap-2.5 rounded-xl px-5 pr-4 text-[15px]')}
          >
            {proof.cta.label}
            <ArrowRightIcon
              aria-hidden
              strokeWidth={1.75}
              className="size-4 transition-transform duration-150 group-hover:translate-x-0.5 motion-reduce:transition-none"
            />
          </a>
        </Reveal>
        <Reveal delay={0.1} className="graphic-stage min-w-0 lg:self-center">
          <GraphScroller>
            <SystemGraph
              stage="harness"
              step={step}
              cycle={cycle}
              active={active}
              label="The full iii graph built through the previous sections, with the harness driving it: each tool call in the ADE session lights up the matching worker as the request runs through the engine."
            />
          </GraphScroller>
        </Reveal>
      </div>
    </Section>
  )
}
