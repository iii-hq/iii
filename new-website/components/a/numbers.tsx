import { ArrowUpRightIcon } from 'lucide-react'

import { Reveal } from '@/components/site/reveal'
import { stagger } from '@/lib/motion'

import { numbers } from './content'
import { MetricFigure } from './metric-figure'
import { Section } from './section'

/**
 * Six figures in a hairline grid, one claim per cell: the figure, what it measures, and the one detail behind
 * it. The grid fades up as one surface (so its hairlines never show through empty cells), then each figure
 * settles on its value in reading order, latencies counting down as a benchmark converges, with the label
 * and detail following a beat later. The footnote says where the benchmarks ran and links to every run.
 */
export function Numbers() {
  const { metrics, footnote } = numbers
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section id="numbers" eyebrow={numbers.eyebrow} title={numbers.title}>
      <Reveal delay={0.1} className="mt-10 lg:mt-14">
        <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-xl border bg-border lg:grid-cols-3">
          {metrics.map((m, i) => {
            /* Figures start once the grid is mostly in; each cell follows the last by one stagger step. */
            const at = 0.3 + i * stagger
            return (
              <div key={m.id} className="flex min-w-0 flex-col bg-card px-5 py-6 sm:px-7 sm:py-8 lg:py-9">
                {/* DOM order keeps dt before dd (valid markup); the figure moves to the top visually. */}
                <dt className="order-2 mt-5 text-[14px] text-foreground leading-snug sm:text-[15px] lg:mt-6">
                  <Reveal delay={at + 0.12}>{m.label}</Reveal>
                </dt>
                <dd className="order-1 font-pixel text-[36px] text-foreground leading-none tabular-nums sm:text-[44px] lg:text-[52px]">
                  <Reveal delay={at}>
                    <MetricFigure value={m.value} unit={m.unit} delay={at + 0.05} />
                  </Reveal>
                </dd>
                <dd className="order-3 mt-1.5 text-pretty text-[13px] text-muted-foreground leading-snug">
                  <Reveal delay={at + 0.16}>{m.detail}</Reveal>
                </dd>
              </div>
            )
          })}
        </dl>
      </Reveal>
      <Reveal delay={0.3 + metrics.length * stagger}>
        <p className="mt-5 max-w-[760px] text-pretty text-[13px] text-muted-foreground leading-relaxed">
          {footnote.setup} {footnote.bench}{' '}
          <a
            href={footnote.link.href}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-0.5 text-foreground underline decoration-border underline-offset-4 transition-colors hover:decoration-foreground"
          >
            {footnote.link.label}
            <ArrowUpRightIcon aria-hidden className="size-3.5" />
          </a>
        </p>
      </Reveal>
    </Section>
  )
}
