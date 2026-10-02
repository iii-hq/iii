import { CountUp } from '@/components/site/count-up'
import { Reveal } from '@/components/site/reveal'
import { Section } from '@/components/site/section'
import { cn } from '@/lib/utils'

import { numbers } from './content'
import type { PageStats } from './stats'

/** Six figures in a hairline grid. Live counts tick up; the doc's XX placeholders are shown as placeholders. */
export function Numbers({ stats }: { stats: PageStats }) {
  const live: Record<string, number | null> = { workers: stats.workers, stars: stats.stars }
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section id="numbers" eyebrow={numbers.eyebrow} title={numbers.title}>
      <Reveal delay={0.1} className="mt-10 lg:mt-14">
        <dl className="grid grid-cols-2 overflow-hidden rounded-xl border bg-card sm:grid-cols-3">
          {numbers.metrics.map((m, i) => {
            const value = m.value ?? live[m.id]
            const placeholder = typeof value === 'string' && value.includes('XX')
            return (
              <div
                key={m.id}
                className={cn(
                  'flex min-w-0 flex-col gap-2 px-5 py-6 sm:px-6 sm:py-8',
                  i % 2 === 0 ? 'border-r' : '',
                  'sm:border-r sm:[&:nth-child(3n)]:border-r-0',
                  i < numbers.metrics.length - 2 ? 'border-b' : '',
                  i < numbers.metrics.length - 3 ? 'sm:border-b' : 'sm:border-b-0',
                )}
              >
                <dd
                  className={cn(
                    'font-pixel text-[32px] leading-none tabular-nums sm:text-[40px]',
                    placeholder ? 'text-muted-foreground/60' : 'text-foreground',
                  )}
                  title={placeholder ? 'Placeholder until the team picks the figure' : undefined}
                >
                  {typeof value === 'number' ? (
                    <CountUp value={value} format={m.id === 'stars' ? 'compact' : 'plain'} />
                  ) : (
                    (value ?? '—')
                  )}
                </dd>
                <dt className="text-[13px] text-muted-foreground leading-snug">{m.label}</dt>
              </div>
            )
          })}
        </dl>
      </Reveal>
    </Section>
  )
}
