import { SectionIntro } from "@/components/landing/section-intro"
import { ArrowRightIcon } from "@/components/site/iconly"
import { buttonVariants } from "@/components/ui/button-variants"
import { links } from "@/lib/site"
import { cn } from "@/lib/utils"
import { EventLink } from "./roadmap-preview/event-link"
import { formatSpecDate, getLatestSpecs, specHref } from "./roadmap-preview/roadmap-feed"

const LOCATION = "landing_tech_specs"

/** Date column width; the title column and the "view all" link both key off it. */
const DATE_COL = "112px"

// A row is one link: date | title + tagline | arrow. The hover surface bleeds
// 16px past the text on both sides so the text columns stay on the grid.
const ROW = cn(
  "group/row -mx-4 grid gap-x-6 gap-y-1.5 rounded-[12px] px-4 py-4 outline-none",
  "sm:grid-cols-[var(--date-col)_minmax(0,1fr)_16px] sm:items-start",
  "transition-[background-color] duration-150 ease-out hover:bg-gray-3",
  "focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-1",
)

/**
 * The newest roadmap specs, fetched from the live iii.dev feed (revalidated
 * hourly). Renders nothing if the feed is unavailable. Titles and taglines are
 * shown as authored: the tech-specs site writes in lowercase on purpose.
 */
export async function RoadmapPreview() {
  const specs = await getLatestSpecs()
  if (!specs.length) return null

  return (
    // biome-ignore lint/correctness/useUniqueElementIds: kept from the Astro page for anchors
    <section id="tech-specs" aria-labelledby="roadmap-title" className="scroll-mt-[116px] py-20 sm:py-24">
      <div className="mx-auto max-w-[1200px] px-5 md:px-6" style={{ "--date-col": DATE_COL } as React.CSSProperties}>
        <SectionIntro eyebrow="Tech specs" titleId="roadmap-title" title="Roadmap.">
          Designs published before they’re built. Read the plan, step through the architecture.
        </SectionIntro>

        {/* Capped so the arrow stays within reach of the text on wide screens. */}
        <ol className="mt-8 flex max-w-[960px] flex-col sm:mt-10">
          {specs.map((spec) => (
            <li key={spec.slug}>
              <EventLink
                href={specHref(spec)}
                event="tech_spec_open"
                params={{ slug: spec.slug, cta_location: LOCATION }}
                className={ROW}
              >
                <time
                  dateTime={spec.date}
                  className="text-[13px] leading-[1.6] text-gray-10 tabular-nums sm:pt-[3px] sm:leading-[1.4]"
                >
                  {formatSpecDate(spec.date)}
                </time>

                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
                    <span className="text-[16px] leading-[1.4] font-medium tracking-[-0.01em] text-gray-12">
                      {spec.title}
                    </span>
                    {spec.hasDeck && (
                      <span className="rounded-[6px] bg-gray-3 px-1.5 py-0.5 text-[11px] leading-[1.5] font-medium text-gray-11 whitespace-nowrap shadow-[inset_0_0_0_1px_var(--gray-5)]">
                        Interactive
                      </span>
                    )}
                  </div>
                  {spec.tagline && (
                    <p className="mt-1 max-w-[600px] text-[14px] leading-[1.6] text-pretty text-gray-11">
                      {spec.tagline}
                    </p>
                  )}
                </div>

                <ArrowRightIcon className="hidden size-4 text-gray-8 transition-[transform,color] duration-150 ease-out group-hover/row:translate-x-0.5 group-hover/row:text-gray-12 sm:mt-[3px] sm:block" />
              </EventLink>
            </li>
          ))}
        </ol>

        {/* Sits in the title column (date column + gap), ghost padding pulled back so the label lines up. */}
        <div className="mt-3 sm:pl-[calc(var(--date-col)+24px)]">
          <EventLink
            href={links.roadmap}
            event="tech_specs_view_all"
            params={{ cta_location: LOCATION }}
            className={cn(buttonVariants({ variant: "ghost" }), "group/cta -mx-3.5 text-gray-12")}
          >
            View all specs
            <ArrowRightIcon className="size-4 transition-transform duration-200 ease-out group-hover/cta:translate-x-0.5" />
          </EventLink>
        </div>
      </div>
    </section>
  )
}
