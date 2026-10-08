import { ArrowUpRightIcon } from 'lucide-react'
import type { Metadata } from 'next'
import { notFound } from 'next/navigation'

import styles from '@/components/article/article.module.css'
import { PageHead } from '@/components/article/page-head'
import { Prose } from '@/components/article/prose'
import { type TocGroup, TocRail } from '@/components/article/toc-rail'
import { wideContainer } from '@/components/landing/section'
import roadmap from '@/components/roadmap/roadmap.module.css'
import { SpecPager } from '@/components/roadmap/spec-pager'
import { Footer } from '@/components/site/footer'
import { SiteHeader } from '@/components/site/site-header'
import { TrackedLink } from '@/components/site/tracked-link'
import { buttonVariants } from '@/components/ui/button'
import { renderMarkdown } from '@/lib/markdown'
import { pageMetadata } from '@/lib/seo'
import { links } from '@/lib/site'
import { formatSpecDate } from '@/lib/spec-dates'
import { docBody, docId, plainTitle, readmeBody } from '@/lib/spec-markdown'
import { getSpec, getSpecs } from '@/lib/specs'
import { cn } from '@/lib/utils'

type Params = { slug: string }

export const dynamicParams = false

const MORE_ID = 'more-in-this-spec'

/* Spec prose has unbreakable runs: paths in inline code (`iii/sdk/…/types.ts:159-…`) and slash-joined words
   (`allowed/forbidden/expose/…`). Left alone, one pushes a 390px page wide. */
const WRAP = 'wrap-break-word [&_:not(pre)>code]:wrap-anywhere'

const action = 'group/cta h-9 px-3'
const actionArrow = 'size-4 transition-transform group-hover/cta:translate-x-0.5 group-hover/cta:-translate-y-0.5'

export function generateStaticParams(): Params[] {
  return getSpecs().map((s) => ({ slug: s.slug }))
}

export async function generateMetadata({ params }: { params: Promise<Params> }): Promise<Metadata> {
  const { slug } = await params
  const spec = getSpec(slug)
  if (!spec) return {}
  const base = pageMetadata({ title: spec.title, description: spec.tagline, path: `/roadmap/${spec.slug}/` })
  return { ...base, openGraph: { ...base.openGraph, type: 'article' } }
}

/** The spec's README on GitHub: the raw markdown, with the repo's own rendering. */
const rawUrl = (slug: string) => `${links.github}/blob/main/tech-specs/${slug}/README.md`

export default async function SpecPage({ params }: { params: Promise<Params> }) {
  const { slug } = await params
  const specs = getSpecs()
  const index = specs.findIndex((s) => s.slug === slug)
  const spec = specs[index]
  if (!spec) notFound()

  const [readme, ...others] = spec.docs
  // The README and the extra docs are independent renders: one round of work, not two.
  const [rendered, extras] = await Promise.all([
    renderMarkdown(readmeBody(readme)),
    Promise.all(
      others.map(async (doc) => {
        const id = docId(doc)
        // Each doc is its own render, so its ids carry the doc's name to stay unique on the page.
        const { content } = await renderMarkdown(docBody(doc), { idPrefix: id })
        return { id, title: plainTitle(doc.title), content }
      }),
    ),
  ])

  const toc: TocGroup[] = [{ items: rendered.headings.filter((h) => h.depth === 2) }]
  if (extras.length) toc.push({ label: 'More in this spec', items: extras.map((d) => ({ id: d.id, text: d.title })) })

  const newer = specs[index - 1]
  const older = specs[index + 1]

  return (
    <>
      <SiteHeader />
      <main>
        <article aria-labelledby="spec-title" className={cn(wideContainer, styles.page)}>
          <PageHead
            eyebrow="Roadmap"
            eyebrowHref={links.roadmap}
            title={spec.title}
            titleId="spec-title"
            description={spec.tagline}
          >
            <div className={styles.meta}>
              <time dateTime={spec.date}>{formatSpecDate(spec.date)}</time>
              {spec.tags.length > 0 ? (
                <>
                  <span aria-hidden="true">·</span>
                  <ul aria-label="Tags" className={roadmap.tags}>
                    {spec.tags.map((tag) => (
                      <li key={tag}>{tag}</li>
                    ))}
                  </ul>
                </>
              ) : null}
              {spec.deckUrl ? <span className={roadmap.chip}>Interactive</span> : null}
            </div>

            <div className="mt-7 flex flex-wrap gap-3">
              {spec.deckUrl ? (
                <TrackedLink
                  href={spec.deckUrl}
                  target="_blank"
                  rel="noopener"
                  params={{ cta_id: 'tech_spec_deck', cta_location: 'tech_spec', slug: spec.slug }}
                  className={cn(buttonVariants(), action)}
                >
                  Open the interactive deck
                  <ArrowUpRightIcon aria-hidden strokeWidth={1.75} className={actionArrow} />
                </TrackedLink>
              ) : null}
              <a
                href={rawUrl(spec.slug)}
                target="_blank"
                rel="noopener"
                className={cn(buttonVariants({ variant: spec.deckUrl ? 'outline' : 'default' }), action)}
              >
                View raw markdown
                <ArrowUpRightIcon aria-hidden strokeWidth={1.75} className={actionArrow} />
              </a>
            </div>
          </PageHead>

          <div className={cn(styles.layout, 'mt-12 sm:mt-16')}>
            <div className={styles.main}>
              <Prose className={WRAP}>{rendered.content}</Prose>

              {extras.length > 0 ? (
                <section aria-labelledby={MORE_ID} className="mt-20 border-t pt-12">
                  <h2 id={MORE_ID} className={cn(styles.eyebrow, 'scroll-mt-28')}>
                    More in this spec
                  </h2>
                  <p className="mt-3 max-w-[560px] text-pretty text-[15px] text-muted-foreground leading-relaxed">
                    The README is the overview. These documents go one level down, in the order they sit in the spec
                    folder.
                  </p>
                  <div className="mt-12 flex flex-col gap-20">
                    {extras.map((doc) => (
                      <section key={doc.id} aria-labelledby={doc.id}>
                        <h2
                          id={doc.id}
                          className="max-w-[68ch] scroll-mt-28 text-balance font-medium text-[26px] text-foreground leading-[1.2] tracking-[-0.02em]"
                        >
                          {doc.title}
                        </h2>
                        <Prose className={cn('mt-6', WRAP)}>{doc.content}</Prose>
                      </section>
                    ))}
                  </div>
                </section>
              ) : null}
            </div>

            <aside className={styles.aside}>
              <TocRail groups={toc} />
            </aside>
          </div>

          <SpecPager newer={newer} older={older} className="mt-20 border-t pt-8" />
        </article>
      </main>
      <Footer container={wideContainer} />
    </>
  )
}
