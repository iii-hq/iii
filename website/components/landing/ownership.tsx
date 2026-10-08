import { ArrowUpRightIcon } from 'lucide-react'

import { CountUp } from '@/components/site/count-up'
import { Reveal } from '@/components/site/reveal'
import { ownership } from './content'
import { OwnershipMove } from './ownership-move'
import { Section } from './section'
import type { PageStats } from './stats'

/**
 * Sits high on the page, right before the Live demo: owning your architecture is the value prop almost no
 * other platform offers. Copy and adoption stats share a row; the move runs full width beneath so the three
 * environments, and the system travelling between them, are big enough to read.
 *
 * Each stat is a link to the public page it was read from (npm, crates.io, Docker Hub, and so on), so the
 * figure is checkable rather than a claim. The link sits on the number and stretches over the whole tile.
 */
export function Ownership({ stats }: { stats: PageStats }) {
  const values: Record<(typeof ownership.stats)[number]['id'], number | null> = {
    workers: stats.workers,
    contributors: stats.contributors,
    ...stats.downloads,
  }
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section id="ownership" eyebrow={ownership.eyebrow} title={ownership.title} lede={ownership.subtitle}>
      <div className="mt-10 grid grid-cols-[minmax(0,1fr)] gap-10 lg:mt-14 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-16">
        <Reveal className="min-w-0">
          <p className="max-w-[560px] text-pretty text-[15px] text-muted-foreground leading-relaxed md:text-[16px]">
            {ownership.copy}
          </p>
        </Reveal>
        <Reveal delay={0.1} className="min-w-0">
          <dl className="grid grid-cols-2 gap-x-6 gap-y-7 sm:grid-cols-3">
            {ownership.stats.map(({ id, label, source, href }) => {
              const value = values[id]
              return (
                <div key={id} className="group relative flex min-w-0 flex-col border-t pt-4">
                  <dt className="order-2 mt-2 flex items-center gap-1 text-[13px] text-muted-foreground leading-snug transition-colors group-hover:text-foreground">
                    {label}
                    <ArrowUpRightIcon
                      aria-hidden
                      className="size-3 shrink-0 text-muted-foreground/60 transition-[translate,color] duration-150 group-hover:translate-x-px group-hover:-translate-y-px group-hover:text-foreground motion-reduce:transition-none"
                    />
                  </dt>
                  <dd className="order-1 font-pixel text-[28px] text-foreground leading-none tabular-nums sm:text-[32px]">
                    <a
                      href={href}
                      target="_blank"
                      rel="noreferrer"
                      title={`Verify on ${source}`}
                      className="outline-none after:absolute after:inset-x-0 after:-inset-y-1 after:rounded-md after:content-[''] focus-visible:after:outline-2 focus-visible:after:outline-foreground focus-visible:after:outline-offset-4"
                    >
                      {value === null ? (
                        '—'
                      ) : (
                        <CountUp
                          value={value}
                          format={id === 'workers' || id === 'contributors' ? 'plain' : 'compact'}
                        />
                      )}
                      <span className="sr-only">, verify on {source}</span>
                    </a>
                  </dd>
                </div>
              )
            })}
          </dl>
        </Reveal>
      </div>
      <Reveal delay={0.15} className="mt-12 lg:mt-16">
        <OwnershipMove />
      </Reveal>
    </Section>
  )
}
