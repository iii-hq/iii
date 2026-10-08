import { ArrowRightIcon, ArrowUpRightIcon } from 'lucide-react'
import type { Metadata } from 'next'
import type { ComponentType } from 'react'
import { IconBot, IconCategory, IconCode, IconPackage, IconTerminal, IconTickSquare } from '@/components/icons/iconly'
import { FinalCta } from '@/components/landing/final-cta'
import { wideContainer } from '@/components/landing/section'
import styles from '@/components/roadmap/roadmap.module.css'
import { Timeline } from '@/components/roadmap/timeline'
import { Footer } from '@/components/site/footer'
import { JsonLd } from '@/components/site/json-ld'
import { PixelHeading } from '@/components/site/pixel-heading'
import { Reveal } from '@/components/site/reveal'
import { SectionRule } from '@/components/site/section-rule'
import { SiteHeader } from '@/components/site/site-header'
import { buttonVariants } from '@/components/ui/button'
import { pageMetadata, site } from '@/lib/seo'
import { dayHeading, monthHeading } from '@/lib/spec-dates'
import { getAllSpecs, type Spec, specHref } from '@/lib/specs'
import { cn } from '@/lib/utils'

/* Copy from the previous iii.dev/roadmap, word for word. */
const HEADLINE = "What we're working on"
const INTRO =
  'The iii roadmap, in public: every priority lands here as a tech spec before it lands as code. Newest first, so the top entry is what we are building right now; everything below it has already shipped into the engine and its workers. Each spec stays readable as markdown, and the big ones earn an interactive deck.'
const DESCRIPTION =
  'The iii roadmap, in public: every priority lands here as a tech spec before it lands as code. Newest first, readable as markdown, steppable as an interactive deck.'

export const metadata: Metadata = pageMetadata({
  title: "iii roadmap: what we're working on",
  description: DESCRIPTION,
  path: '/roadmap/',
})

/** One glyph per spec, from its first recognised tag (Iconly Bold, like every non-arrow icon on the site). */
const GLYPHS: [string, ComponentType<{ className?: string }>][] = [
  ['agents', IconBot],
  ['security', IconTickSquare],
  ['codegen', IconCode],
  ['compose', IconPackage],
  ['console', IconTerminal],
]
const glyphFor = (tags: string[]) => GLYPHS.find(([tag]) => tags.includes(tag))?.[1] ?? IconCategory

function groupByMonth(specs: Spec[]) {
  const groups: { key: string; label: string; specs: Spec[] }[] = []
  for (const spec of specs) {
    const key = spec.date.slice(0, 7)
    const last = groups.at(-1)
    if (last?.key === key) last.specs.push(spec)
    else groups.push({ key, label: monthHeading(spec.date), specs: [spec] })
  }
  return groups
}

function SpecCard({ spec }: { spec: Spec }) {
  const live = spec.status === 'live'
  const Glyph = glyphFor(spec.tags)
  const Arrow = live ? ArrowRightIcon : ArrowUpRightIcon
  return (
    <a
      href={specHref(spec)}
      className={styles.card}
      {...(live ? {} : { target: '_blank', rel: 'noopener noreferrer' })}
    >
      <div className={styles.cardTop}>
        <span className={styles.status} data-live={live}>
          <span aria-hidden />
          {live ? 'Live' : 'In draft'}
        </span>
        <time dateTime={spec.date} className={styles.cardDate}>
          {dayHeading(spec.date)}
        </time>
        {spec.deckUrl ? <span className={styles.chip}>Interactive</span> : null}
        <span className={styles.glyph} aria-hidden>
          <Glyph className="size-4" />
        </span>
      </div>
      <h3 className={styles.cardTitle}>{spec.title}</h3>
      {spec.tagline ? <p className={styles.cardTagline}>{spec.tagline}</p> : null}
      <div className={styles.cardFoot}>
        <ul aria-label="Tags" className={styles.tags}>
          {spec.tags.map((tag) => (
            <li key={tag}>{tag}</li>
          ))}
        </ul>
        <Arrow aria-hidden strokeWidth={1.75} className={styles.cardArrow} />
      </div>
    </a>
  )
}

export default function RoadmapPage() {
  const specs = getAllSpecs()
  const groups = groupByMonth(specs)
  const live = specs.filter((spec) => spec.status === 'live')
  const stats = [
    { value: String(specs.length), label: 'Specs' },
    { value: String(specs.filter((spec) => spec.deckUrl).length), label: 'Interactive decks' },
    { value: live[0] ? dayHeading(live[0].date) : '–', label: 'Latest shipped' },
    { value: String(specs.length - live.length), label: 'In draft' },
  ]
  const timeline = specs.map((spec) => ({
    slug: spec.slug,
    title: spec.title,
    date: spec.date,
    day: dayHeading(spec.date),
    status: spec.status,
  }))
  const itemList = {
    '@context': 'https://schema.org',
    '@type': 'ItemList',
    name: 'iii roadmap',
    url: `${site.url}/roadmap`,
    itemListElement: specs.map((spec, i) => ({
      '@type': 'ListItem',
      position: i + 1,
      name: spec.title,
      url: new URL(specHref(spec), site.url).href,
    })),
  }

  return (
    <>
      <SiteHeader />
      <main>
        <section aria-labelledby="roadmap-title" className="landing-section relative">
          <div className={cn(wideContainer, styles.hero)}>
            <div className={styles.heroCopy}>
              <Reveal>
                <p className={styles.eyebrow}>Roadmap</p>
              </Reveal>
              {/* biome-ignore lint/correctness/useUniqueElementIds: One page heading; the section is labelled by it. */}
              <PixelHeading as="h1" id="roadmap-title" className={styles.heroTitle}>
                {HEADLINE}
              </PixelHeading>
              <Reveal delay={0.1}>
                <p className={styles.heroLede}>{INTRO}</p>
              </Reveal>
              <Reveal delay={0.15}>
                <dl className={styles.stats}>
                  {stats.map((stat) => (
                    <div key={stat.label} className={styles.stat}>
                      <dt>{stat.label}</dt>
                      <dd>{stat.value}</dd>
                    </div>
                  ))}
                </dl>
              </Reveal>
              <Reveal delay={0.2} className={styles.actions}>
                <a href="/roadmap/index.json" className={cn(buttonVariants({ variant: 'outline' }), styles.action)}>
                  JSON feed
                  <ArrowRightIcon aria-hidden strokeWidth={1.75} className="size-4" />
                </a>
                <a
                  href="https://github.com/iii-hq/iii/tree/main/tech-specs"
                  target="_blank"
                  rel="noopener noreferrer"
                  className={cn(buttonVariants({ variant: 'outline' }), styles.action)}
                >
                  Specs on GitHub
                  <ArrowUpRightIcon aria-hidden strokeWidth={1.75} className="size-4" />
                </a>
              </Reveal>
            </div>
            <Reveal delay={0.15} className={styles.heroFigure}>
              <Timeline specs={timeline} />
            </Reveal>
          </div>
        </section>

        <section aria-label="Specs by month" className="landing-section relative">
          <SectionRule />
          <div className={cn(wideContainer, styles.months)}>
            {groups.map((group) => (
              <section key={group.key} aria-labelledby={`month-${group.key}`} className={styles.month}>
                <div className={styles.monthHead}>
                  <h2 id={`month-${group.key}`}>{group.label}</h2>
                  <p>
                    {group.specs.length} {group.specs.length === 1 ? 'spec' : 'specs'}
                  </p>
                </div>
                <ul className={styles.grid}>
                  {group.specs.map((spec, i) => (
                    <li key={spec.slug}>
                      <Reveal delay={i * 0.05} className="h-full">
                        <SpecCard spec={spec} />
                      </Reveal>
                    </li>
                  ))}
                </ul>
              </section>
            ))}
          </div>
        </section>

        <FinalCta />
      </main>
      <Footer container={wideContainer} />
      <JsonLd data={itemList} />
    </>
  )
}
