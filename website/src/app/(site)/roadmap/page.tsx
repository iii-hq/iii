import type { Metadata } from "next"
import type { CSSProperties } from "react"
import { EventLink } from "@/components/landing/roadmap-preview/event-link"
import { InView } from "@/components/motion/in-view"
import { RoadmapGraph } from "@/components/roadmap/roadmap-graph"
import { SpecCard } from "@/components/roadmap/spec-card"
import { ArrowRightIcon, ArrowUpRightIcon } from "@/components/site/iconly"
import { PageShell } from "@/components/site/page-shell"
import { buttonVariants } from "@/components/ui/button-variants"
import { dayHeading, monthHeading, monthKey } from "@/lib/spec-dates"
import { getAllSpecs, type Spec } from "@/lib/specs"
import { cn } from "@/lib/utils"

// Copy is the live roadmap page's, as it reads on iii.dev today, in the sentence case the
// rest of this site uses for its own UI voice. Spec titles and taglines stay as authored.
const TITLE = "iii — what we're working on"
const HEADLINE = "What we're working on"
const INTRO =
  "The iii roadmap, in public: every priority lands here as a tech spec before it lands as code. Newest first, so the top entry is what we are building right now; everything below it has already shipped into the engine and its workers. Each spec stays readable as markdown, and the big ones earn an interactive deck."
const DESCRIPTION =
  "the iii roadmap, in public: every priority lands here as a tech spec before it lands as code — newest first, readable as markdown, steppable as an interactive deck."

export const metadata: Metadata = {
  title: TITLE,
  description: DESCRIPTION,
  alternates: { canonical: "/roadmap/" },
  openGraph: {
    type: "website",
    url: "/roadmap/",
    title: TITLE,
    description: DESCRIPTION,
    images: [{ url: "/og-image.png", width: 1200, height: 630, type: "image/png" }],
  },
  twitter: { card: "summary_large_image", title: TITLE, description: DESCRIPTION, images: ["/og-image.png"] },
}

/** Explicit delay for a block in the opening cascade (see `.intro` in globals.css). */
const at = (ms: number) => ({ "--d": `${ms}ms` }) as CSSProperties

/** The four figures the live page shows under its intro. */
function Stats({ specs }: { specs: Spec[] }) {
  const live = specs.filter((s) => s.status === "live")
  const latest = live[0]
  const items = [
    { value: String(specs.length), label: "specs" },
    { value: String(specs.filter((s) => s.deckUrl).length), label: "interactive" },
    { value: latest ? dayHeading(latest.date) : "—", label: "latest" },
    { value: String(specs.length - live.length), label: "in draft" },
  ]
  return (
    // Phones: two equal columns across the copy column. Wider: compact pills that wrap.
    <dl className="grid grid-cols-2 gap-2 sm:flex sm:flex-wrap">
      {items.map((it) => (
        <div
          key={it.label}
          className="flex min-w-0 flex-col gap-1 rounded-[12px] bg-gray-2 px-4 py-3 shadow-panel sm:min-w-[112px]"
        >
          <dd className="order-1 text-[20px] leading-none font-medium tracking-[-0.02em] text-gray-12 tabular-nums">
            {it.value}
          </dd>
          <dt className="order-2 font-mono text-[11px] tracking-[0.04em] text-gray-9">{it.label}</dt>
        </div>
      ))}
    </dl>
  )
}

type Group = { key: string; label: string; specs: Spec[] }

function groupByMonth(specs: Spec[]): Group[] {
  const groups: Group[] = []
  for (const spec of specs) {
    const key = monthKey(spec.date)
    const last = groups[groups.length - 1]
    if (last && last.key === key) last.specs.push(spec)
    else groups.push({ key, label: monthHeading(spec.date), specs: [spec] })
  }
  return groups
}

export default function RoadmapPage() {
  const specs = getAllSpecs()
  const groups = groupByMonth(specs)

  return (
    <PageShell>
      {/* Head: the landing hero's grid. Copy on the leading edge, the timeline drawing on the right. */}
      <section aria-labelledby="roadmap-title" className="pt-12 pb-16 sm:pt-20 sm:pb-20">
        <div className="mx-auto max-w-[1200px] px-5 md:px-6">
          <div className="grid grid-cols-[minmax(0,1fr)] items-center gap-12 lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)] lg:gap-16">
            <div className="max-w-[640px] min-w-0">
              <p style={at(40)} className="intro text-[13px] font-medium tracking-[0.02em] text-gray-10">
                Roadmap
              </p>
              {/* biome-ignore lint/correctness/useUniqueElementIds: one page heading; the section is labelled by it */}
              <h1
                id="roadmap-title"
                style={at(120)}
                className="intro mt-5 text-[clamp(30px,7.6vw,36px)] leading-[1.06] font-medium tracking-[-0.04em] text-gray-12 sm:text-[clamp(30px,4.2vw,50px)]"
              >
                {HEADLINE}
              </h1>
              <p
                style={at(240)}
                className="intro mt-5 max-w-[560px] text-[15px] leading-[1.65] text-pretty text-gray-11"
              >
                {INTRO}
              </p>
              <div style={at(360)} className="intro mt-8">
                <Stats specs={specs} />
              </div>
              <div style={at(440)} className="intro mt-6 flex flex-wrap gap-2">
                <EventLink
                  href="/roadmap/index.json"
                  event="tech_specs_feed"
                  params={{ cta_location: "roadmap_index" }}
                  className={cn(buttonVariants({ variant: "outline" }), "group/cta")}
                >
                  JSON feed
                  <ArrowRightIcon className="size-4 text-gray-10 transition-transform duration-200 ease-out group-hover/cta:translate-x-0.5" />
                </EventLink>
                <a
                  href="https://github.com/iii-hq/iii/tree/main/tech-specs"
                  target="_blank"
                  rel="noopener noreferrer"
                  className={cn(buttonVariants({ variant: "outline" }), "group/cta")}
                >
                  Specs on GitHub
                  <ArrowUpRightIcon className="size-4 text-gray-10 transition-transform duration-200 ease-out group-hover/cta:translate-x-0.5 group-hover/cta:-translate-y-0.5" />
                </a>
              </div>
            </div>

            <InView className="intro-slow text-gray-12 max-lg:hidden" style={at(520)}>
              <RoadmapGraph specs={specs} className="h-auto w-full" />
            </InView>
          </div>
        </div>
      </section>

      {/* The specs, newest first, a month at a time, as cards on the page grid. */}
      <div className="pb-20 sm:pb-24">
        <div className="mx-auto flex max-w-[1200px] flex-col gap-16 px-5 md:px-6">
          {groups.map((group) => {
            const headingId = `month-${group.key}`
            const count = group.specs.length
            return (
              <section key={group.key} aria-labelledby={headingId}>
                <div className="flex items-baseline justify-between gap-6">
                  <h2 id={headingId} className="text-[20px] leading-[1.3] font-medium tracking-[-0.02em] text-gray-12">
                    {group.label}
                  </h2>
                  <p className="font-mono text-[12px] text-gray-9 tabular-nums">
                    {count} {count === 1 ? "spec" : "specs"}
                  </p>
                </div>
                <ul className="mt-6 grid gap-4 lg:grid-cols-2">
                  {group.specs.map((spec) => (
                    <li key={spec.slug}>
                      <InView className="h-full">
                        <SpecCard spec={spec} />
                      </InView>
                    </li>
                  ))}
                </ul>
              </section>
            )
          })}
        </div>
      </div>
    </PageShell>
  )
}
