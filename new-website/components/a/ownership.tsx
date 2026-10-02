import { CountUp } from '@/components/site/count-up'
import { Reveal } from '@/components/site/reveal'
import { ownership } from './content'
import { OwnershipMove } from './ownership-move'
import { Section } from './section'
import type { PageStats } from './stats'

export function Ownership({ stats }: { stats: PageStats }) {
  const facts: [string, number | null][] = [
    ['GitHub stars', stats.stars],
    ['Contributors', stats.contributors],
    ['Workers', stats.workers],
  ]
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section id="ownership" eyebrow={ownership.eyebrow} title={ownership.title} lede={ownership.subtitle}>
      <div className="mt-10 grid grid-cols-[minmax(0,1fr)] gap-10 lg:mt-14 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-16">
        <Reveal className="flex min-w-0 flex-col gap-6">
          <p className="max-w-[560px] text-pretty text-[15px] text-muted-foreground leading-relaxed">
            {ownership.copy}
          </p>
          <p className="font-sans text-[13px] text-foreground">{ownership.environments.join(' · ')}</p>
          <dl className="grid grid-cols-3 gap-4 border-t pt-5">
            {facts.map(([label, value]) => (
              <div key={label}>
                <dd className="font-pixel text-[26px] text-foreground leading-none tabular-nums sm:text-[30px]">
                  {value === null ? (
                    '—'
                  ) : (
                    <CountUp value={value} format={label === 'GitHub stars' ? 'compact' : 'plain'} />
                  )}
                </dd>
                <dt className="mt-1.5 text-[13px] text-muted-foreground">{label}</dt>
              </div>
            ))}
          </dl>
        </Reveal>
        <Reveal delay={0.1} className="min-w-0">
          <OwnershipMove />
        </Reveal>
      </div>
    </Section>
  )
}
