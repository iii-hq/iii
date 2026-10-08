import type { Metadata } from 'next'

import { FinalCta } from '@/components/landing/final-cta'
import { wideContainer } from '@/components/landing/section'
import { Collapse } from '@/components/manifesto/collapse'
import { hero, type Inline, type Statement, statements } from '@/components/manifesto/data'
import styles from '@/components/manifesto/manifesto.module.css'
import { Motif } from '@/components/manifesto/motifs'
import { ReadingIndex } from '@/components/manifesto/reading-index'
import { Footer } from '@/components/site/footer'
import { JsonLd } from '@/components/site/json-ld'
import { PixelHeading } from '@/components/site/pixel-heading'
import { Reveal } from '@/components/site/reveal'
import { SectionRule } from '@/components/site/section-rule'
import { SiteHeader } from '@/components/site/site-header'
import { pageMetadata, site } from '@/lib/seo'
import { cn } from '@/lib/utils'

/* Title and description from the live iii.dev/manifesto. */
const description =
  'The iii manifesto. How software can be collapsed into three primitives: Worker, Trigger, Function. Linear scaling, zero integration cost, agent-native.'
const lede =
  'How software collapses into three primitives: Worker, Trigger, Function. Linear scaling, zero integration cost, agent-native.'

export const metadata: Metadata = pageMetadata({
  title: 'The iii manifesto: collapse the categories',
  description,
  path: '/manifesto',
})

const articleJsonLd = {
  '@context': 'https://schema.org',
  '@type': 'Article',
  headline: 'The iii manifesto',
  description,
  url: `${site.url}/manifesto`,
  inLanguage: 'en',
  author: { '@type': 'Organization', name: 'III, Inc.', url: `${site.url}/` },
  publisher: { '@id': `${site.url}/#organization` },
}

/** Body copy with the source's italic phrases kept. */
function Body({ inline }: { inline: Inline[] }) {
  return inline.map((run) =>
    typeof run === 'string' ? (
      <span key={run}>{run}</span>
    ) : (
      <i key={`i:${run.i}`} className="text-foreground">
        {run.i}
      </i>
    ),
  )
}

function StatementBlock({ statement, index }: { statement: Statement; index: number }) {
  const titleId = `${statement.id}-title`
  return (
    <section id={statement.id} aria-labelledby={titleId} className={styles.statement}>
      <Motif index={index} className={styles.statementMotif} />
      <Reveal className={styles.statementText}>
        <h2 id={titleId} className={styles.statementTitle}>
          {statement.code ? (
            <code className={styles.accent}>{statement.accent}</code>
          ) : (
            <span className={styles.accent}>{statement.accent}</span>
          )}{' '}
          {statement.heavy}
          {statement.ghost ? <span className={styles.ghost}> {statement.ghost}</span> : null}
        </h2>
        <p className={styles.statementBody}>
          <Body inline={statement.body} />
        </p>
      </Reveal>
    </section>
  )
}

export default function ManifestoPage() {
  return (
    <>
      <SiteHeader />
      <main>
        {/* Head: the hero's three lines, and the argument they make, drawn. */}
        <section aria-labelledby="manifesto-title" className="landing-section relative">
          <div className={cn(wideContainer, styles.hero)}>
            <div className={styles.heroCopy}>
              <Reveal>
                <p className={styles.eyebrow}>Manifesto</p>
              </Reveal>
              {/* biome-ignore lint/correctness/useUniqueElementIds: One page heading; the section is labelled by it. */}
              <PixelHeading as="h1" id="manifesto-title" className={styles.heroTitle}>
                <span className="block">{hero.problem}</span>
                <span className="block">
                  {hero.verb} {hero.answer}
                </span>
              </PixelHeading>
              <Reveal delay={0.1}>
                <p className={styles.heroLede}>{lede}</p>
              </Reveal>
            </div>
            <Reveal delay={0.15} className={styles.heroFigure}>
              <Collapse />
            </Reveal>
          </div>
        </section>

        {/* The twelve statements, with an index that follows the reading. */}
        <section aria-label="The manifesto" className="landing-section relative">
          <SectionRule />
          <div className={cn(wideContainer, styles.body)}>
            <aside className={styles.indexColumn}>
              <ReadingIndex statements={statements} />
            </aside>
            <article className={styles.statements}>
              {statements.map((statement, index) => (
                <StatementBlock key={statement.id} statement={statement} index={index} />
              ))}
            </article>
          </div>
        </section>

        <FinalCta />
      </main>
      <Footer container={wideContainer} />
      <JsonLd data={articleJsonLd} />
    </>
  )
}
