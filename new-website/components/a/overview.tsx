'use client'

import { AnimatePresence, motion } from 'motion/react'

import { useGraphicLoop } from '@/components/graphics/use-graphic-loop'
import { Reveal } from '@/components/site/reveal'
import { easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'
import { overview } from './content'
import { WORKERS } from './graph/model'
import { GraphScroller } from './graph/scroller'
import { SystemGraph } from './graph/system-graph'
import { useStageClock } from './graph/use-stage-clock'
import { Section } from './section'

const ORIGIN_CAPTION: Record<string, string> = {
  registry: 'installed from the registry',
  written: 'written by an agent',
  human: 'added by you',
}

/** Which Overview "pull" row lights up for which worker's arrival. */
const PULL_FOR_NODE: Record<string, number> = { browser: 0, extract: 1, pg: 2 }

export function Overview() {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const { step, cycle } = useStageClock('foundation', active)
  const arriving = step < WORKERS.length ? WORKERS[step] : null
  const caption = arriving
    ? `${arriving.worker} joins · ${ORIGIN_CAPTION[arriving.origin]}`
    : step === 7
      ? 'Local, Cloud, Browser and Edge merge into one engine'
      : 'one execution graph · every call passes through the engine'
  const litPull = arriving ? PULL_FOR_NODE[arriving.id] : -1

  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section id="overview" eyebrow={overview.eyebrow} title={overview.title} lede={overview.subtitle}>
      <div
        ref={ref}
        className="mt-10 grid grid-cols-[minmax(0,1fr)] gap-10 lg:mt-14 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-16"
      >
        <Reveal className="flex min-w-0 flex-col gap-7">
          <dl className="flex flex-col gap-6">
            {[
              ['The problem', overview.problem],
              ['The cost', overview.agitation],
              ['With iii', overview.solution],
            ].map(([term, body], i) => (
              <div key={term} className={cn('border-l pl-4', i === 2 ? 'border-foreground' : 'border-border')}>
                <dt className="font-sans text-[13px] text-muted-foreground uppercase tracking-[0.08em]">{term}</dt>
                <dd
                  className={cn(
                    'mt-1.5 text-pretty text-[15px] leading-relaxed',
                    i === 2 ? 'text-foreground' : 'text-muted-foreground',
                  )}
                >
                  {body}
                </dd>
              </div>
            ))}
          </dl>
          <div className="rounded-xl border bg-card">
            <p className="border-b px-4 py-2.5 font-sans text-[13px] text-muted-foreground uppercase tracking-[0.08em]">
              How workers get in
            </p>
            <ul className="divide-y">
              {overview.pulls.map((pull, i) => (
                <li
                  key={pull.what}
                  className={cn(
                    'flex items-center gap-3 px-4 py-2.5 font-sans text-[13px] transition-colors duration-300',
                    litPull === i ? 'bg-faint text-foreground' : 'text-muted-foreground',
                  )}
                >
                  <span
                    className={cn(
                      'flex h-5 shrink-0 items-center rounded border px-1.5 text-[10px]',
                      litPull === i ? 'border-hero-accent text-hero-accent' : '',
                    )}
                  >
                    {pull.who}
                  </span>
                  <span className="truncate">{pull.verb}</span>
                  <span className="ml-auto shrink-0 font-mono text-foreground">{pull.what}</span>
                </li>
              ))}
            </ul>
          </div>
        </Reveal>
        <Reveal delay={0.1} className="graphic-stage min-w-0">
          <GraphScroller>
            <SystemGraph
              stage="foundation"
              step={step}
              cycle={cycle}
              active={active}
              label="Seven workers in different languages (an agent, a browser, Rust, a database, TypeScript, Python and a GPU) join one iii engine one by one. Below the engine, Local, Cloud, Browser and Edge merge into a single trunk: one execution graph."
            />
          </GraphScroller>
          <div
            aria-hidden
            className="mt-3 flex h-5 items-center justify-center overflow-hidden text-center font-sans text-[13px] text-muted-foreground"
          >
            <AnimatePresence mode="wait" initial={false}>
              <motion.span
                key={caption}
                className="truncate"
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -6 }}
                transition={{ duration: 0.3, ease: easeOut }}
              >
                {caption}
              </motion.span>
            </AnimatePresence>
          </div>
        </Reveal>
      </div>
    </Section>
  )
}
