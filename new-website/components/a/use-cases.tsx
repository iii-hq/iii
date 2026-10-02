'use client'

import { ArrowRightIcon } from 'lucide-react'
import { motion } from 'motion/react'
import { useId, useState } from 'react'

import { Reveal } from '@/components/site/reveal'
import { spring } from '@/lib/motion'
import { cn } from '@/lib/utils'
import { useCases } from './content'
import { MiniGraph } from './mini-graph'
import { Section } from './section'

export function UseCases() {
  const id = useId()
  const [index, setIndex] = useState(0)
  const tab = useCases.tabs[index]

  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section id="use-cases" eyebrow={useCases.eyebrow} title={useCases.title} lede={useCases.subtitle}>
      <Reveal delay={0.1} className="mt-10 lg:mt-14">
        <div
          role="tablist"
          aria-label="Use cases"
          className="-mx-[5vw] flex gap-1 overflow-x-auto border-b px-[5vw] [mask-image:linear-gradient(to_right,transparent,#000_5vw,#000_calc(100%-5vw),transparent)] [scrollbar-width:none] lg:mx-0 lg:px-0 lg:[mask-image:none] [&::-webkit-scrollbar]:hidden"
        >
          {useCases.tabs.map((t, i) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={i === index}
              aria-controls={`${id}-panel`}
              onClick={() => setIndex(i)}
              className={cn(
                'relative -mb-px h-11 shrink-0 whitespace-nowrap px-4 font-medium text-[14px] outline-none transition-colors focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2',
                i === index ? 'text-foreground' : 'text-muted-foreground hover:text-foreground',
              )}
            >
              {t.label}
              {i === index ? (
                <motion.span
                  layoutId={`${id}-indicator`}
                  aria-hidden
                  className="absolute inset-x-0 bottom-0 h-px bg-foreground"
                  transition={spring.snappy}
                />
              ) : null}
            </button>
          ))}
        </div>
        <div
          id={`${id}-panel`}
          role="tabpanel"
          key={tab.id}
          className="swap-in mt-8 grid grid-cols-[minmax(0,1fr)] gap-8 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-14"
        >
          <div className="flex min-w-0 flex-col gap-5">
            <div className="border-l pl-4">
              <p className="font-sans text-[13px] text-muted-foreground uppercase tracking-[0.08em]">The problem</p>
              <p className="mt-1.5 text-[15px] text-muted-foreground leading-relaxed">{tab.problem}</p>
            </div>
            <div className="border-foreground border-l pl-4">
              <p className="font-sans text-[13px] text-muted-foreground uppercase tracking-[0.08em]">With iii</p>
              <p className="mt-1.5 text-pretty text-[15px] text-foreground leading-relaxed">{tab.solution}</p>
            </div>
            <ul className="flex flex-wrap gap-1.5">
              {tab.fns.map((fn) => (
                <li key={fn} className="rounded-md border bg-card px-2 py-1 font-mono text-[13px] text-foreground">
                  {fn}
                </li>
              ))}
            </ul>
            <a
              href={tab.href}
              className="group inline-flex w-fit items-center gap-1.5 font-medium text-[14px] text-foreground underline-offset-4 outline-none hover:underline focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-4"
            >
              Read the {tab.label} use case
              <ArrowRightIcon
                aria-hidden
                className="size-3.5 transition-transform duration-150 group-hover:translate-x-0.5"
              />
            </a>
          </div>
          <div className="min-w-0 lg:self-center">
            <MiniGraph
              fns={tab.fns}
              label={`The iii graph for ${tab.label}: ${tab.fns.join(', ')}, each wired to one engine, with a call running through them in turn.`}
            />
          </div>
        </div>
      </Reveal>
    </Section>
  )
}
