import type { Metadata } from "next"
import { notFound } from "next/navigation"
import { DeckChip, TagChip } from "@/components/roadmap/chips"
import { PageHead } from "@/components/roadmap/page-head"
import { SpecPager } from "@/components/roadmap/spec-pager"
import { ArrowUpRightIcon } from "@/components/site/iconly"
import { PageShell } from "@/components/site/page-shell"
import { Prose } from "@/components/site/prose"
import { type TocGroup, TocRail } from "@/components/site/toc-rail"
import { TrackedLink } from "@/components/site/tracked-link"
import { buttonVariants } from "@/components/ui/button-variants"
import { renderMarkdown } from "@/lib/markdown"
import { links } from "@/lib/site"
import { formatSpecDate } from "@/lib/spec-dates"
import { docBody, docId, plainTitle, readmeBody } from "@/lib/spec-markdown"
import { getSpec, getSpecs } from "@/lib/specs"
import { cn } from "@/lib/utils"

type Params = { slug: string }

export const dynamicParams = false

const LOCATION = "tech_spec"
const MORE_ID = "more-in-this-spec"

// Spec prose has unbreakable runs: paths in inline code (`iii/sdk/…/types.ts:159-…`) and
// slash-joined words (`allowed/forbidden/expose/…`). Left alone, one pushes a 390px page wide.
// `.prose` should own this (see report).
const WRAP = "wrap-break-word [&_:not(pre)>code]:wrap-anywhere"

export function generateStaticParams(): Params[] {
  return getSpecs().map((s) => ({ slug: s.slug }))
}

export async function generateMetadata({ params }: { params: Promise<Params> }): Promise<Metadata> {
  const { slug } = await params
  const spec = getSpec(slug)
  if (!spec) return {}
  const url = `/roadmap/${spec.slug}`
  return {
    title: spec.title,
    description: spec.tagline,
    alternates: { canonical: url },
    openGraph: {
      type: "article",
      url,
      title: spec.title,
      description: spec.tagline,
      images: [{ url: "/og-image.png", width: 1200, height: 630, type: "image/png" }],
    },
    twitter: { card: "summary_large_image", title: spec.title, description: spec.tagline },
  }
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
  if (extras.length) toc.push({ label: "More in this spec", items: extras.map((d) => ({ id: d.id, text: d.title })) })

  const newer = specs[index - 1]
  const older = specs[index + 1]

  return (
    <PageShell>
      <article aria-labelledby="spec-title" className="pt-16 pb-24 sm:pt-24 sm:pb-32">
        <div className="mx-auto max-w-[1200px] px-5 md:px-6">
          <PageHead eyebrow="Roadmap" title={spec.title} titleId="spec-title" description={spec.tagline}>
            {/* Meta: date · tags. */}
            <div className="mt-5 flex flex-wrap items-center gap-x-2.5 gap-y-2 text-[13px] leading-[1.5] text-gray-10">
              <time dateTime={spec.date} className="tabular-nums">
                {formatSpecDate(spec.date)}
              </time>
              {spec.tags.length > 0 && (
                <>
                  <span aria-hidden="true">·</span>
                  <ul aria-label="Tags" className="flex flex-wrap gap-1.5">
                    {spec.tags.map((tag) => (
                      <li key={tag}>
                        <TagChip>{tag}</TagChip>
                      </li>
                    ))}
                  </ul>
                </>
              )}
              {spec.deckUrl && <DeckChip />}
            </div>

            <div className="mt-7 flex flex-wrap gap-3">
              {spec.deckUrl && (
                <TrackedLink
                  href={spec.deckUrl}
                  target="_blank"
                  rel="noopener"
                  cta={{ cta_id: "tech_spec_deck", cta_location: LOCATION, slug: spec.slug }}
                  className={cn(buttonVariants({ variant: "default", size: "lg" }), "group/cta")}
                >
                  Open the interactive deck
                  <ArrowUpRightIcon className="size-4 transition-transform duration-200 ease-out group-hover/cta:translate-x-0.5 group-hover/cta:-translate-y-0.5" />
                </TrackedLink>
              )}
              <a
                href={rawUrl(spec.slug)}
                target="_blank"
                rel="noopener"
                className={cn(
                  buttonVariants({ variant: spec.deckUrl ? "outline" : "default", size: "lg" }),
                  "group/cta",
                )}
              >
                View raw markdown
                <ArrowUpRightIcon className="size-4 transition-transform duration-200 ease-out group-hover/cta:translate-x-0.5 group-hover/cta:-translate-y-0.5" />
              </a>
            </div>
          </PageHead>

          <div className="mt-12 sm:mt-16 lg:grid lg:grid-cols-[minmax(0,1fr)_200px] lg:gap-x-16 xl:gap-x-24">
            <div className="min-w-0">
              <Prose className={WRAP}>{rendered.content}</Prose>

              {extras.length > 0 && (
                <section aria-labelledby={MORE_ID} className="mt-20 border-t border-gray-5 pt-12">
                  <h2 id={MORE_ID} className="scroll-mt-24 text-[13px] font-medium tracking-[0.02em] text-gray-10">
                    More in this spec
                  </h2>
                  <p className="mt-2 max-w-[560px] text-[15px] leading-[1.6] text-pretty text-gray-11">
                    The README is the overview. These documents go one level down, in the order they sit in the spec
                    folder.
                  </p>
                  <div className="mt-12 flex flex-col gap-20">
                    {extras.map((doc) => (
                      <section key={doc.id} aria-labelledby={doc.id}>
                        <h2
                          id={doc.id}
                          className="max-w-[68ch] scroll-mt-24 text-[26px] leading-[1.2] font-medium tracking-[-0.02em] text-balance text-gray-12"
                        >
                          {doc.title}
                        </h2>
                        <Prose className={cn("mt-6", WRAP)}>{doc.content}</Prose>
                      </section>
                    ))}
                  </div>
                </section>
              )}
            </div>

            <aside className="hidden lg:block">
              <TocRail title="On this page" groups={toc} className="sticky top-24" />
            </aside>
          </div>

          <SpecPager newer={newer} older={older} className="mt-20 border-t border-gray-5 pt-8" />
        </div>
      </article>
    </PageShell>
  )
}
