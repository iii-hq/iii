import { EventLink } from "@/components/landing/roadmap-preview/event-link"
import { ArrowRightIcon, ArrowUpRightIcon } from "@/components/site/iconly"
import { dayHeading } from "@/lib/spec-dates"
import type { Spec } from "@/lib/specs"
import { cn } from "@/lib/utils"
import { SpecGlyph } from "./spec-glyph"

const LOCATION = "roadmap_index"
const GITHUB_SPECS = "https://github.com/iii-hq/iii/tree/main/tech-specs"

/** Status as on a build board: a solid dot for what's live, a ring for what's in draft. */
export function Status({ status }: { status: Spec["status"] }) {
  const live = status === "live"
  return (
    <span className="inline-flex h-6 items-center gap-2 rounded-[6px] bg-gray-3 px-2 font-mono text-[12px] text-gray-11">
      <span
        aria-hidden="true"
        className={cn("size-1.5 rounded-full", live ? "bg-gray-12" : "shadow-[inset_0_0_0_1px_var(--gray-9)]")}
      />
      {live ? "live" : "in draft"}
    </span>
  )
}

/**
 * One spec as a card in the landing page's card language (rounded surface,
 * one hairline ring, ring brightens on hover). Live specs link to their page;
 * drafts link to their folder on GitHub, since they have no page yet.
 */
export function SpecCard({ spec }: { spec: Spec }) {
  const live = spec.status === "live"
  const card = cn(
    "group/card flex h-full flex-col rounded-[14px] bg-gray-2 p-6 shadow-panel outline-none",
    "transition-[box-shadow] duration-200 ease-out hover:shadow-[var(--shadow-panel),inset_0_0_0_1px_var(--gray-8)]",
    "focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-1",
  )
  const inner = (
    <>
      <div className="flex items-start justify-between gap-4">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <Status status={spec.status} />
          <time dateTime={spec.date} className="text-[13px] text-gray-10 tabular-nums">
            {dayHeading(spec.date)}
          </time>
          {spec.deckUrl && (
            <span className="rounded-[6px] bg-gray-3 px-1.5 py-0.5 text-[11px] leading-[1.5] font-medium text-gray-11">
              Interactive
            </span>
          )}
        </div>
        {/* The spec's glyph, from its tags; draws in with the card. */}
        <SpecGlyph tags={spec.tags} className="-mt-1 -mr-1 shrink-0 text-gray-11" />
      </div>
      <h3 className="mt-5 text-[18px] leading-[1.35] font-medium tracking-[-0.015em] text-balance text-gray-12">
        {spec.title}
      </h3>
      {spec.tagline && (
        <p className="mt-2 max-w-[52ch] text-[14px] leading-[1.6] text-pretty text-gray-11">{spec.tagline}</p>
      )}
      <div className="mt-auto flex items-center justify-between gap-4 pt-6">
        <ul aria-label="Tags" className="flex flex-wrap gap-x-3 gap-y-1 font-mono text-[12px] text-gray-9">
          {spec.tags.map((tag) => (
            <li key={tag}>{tag}</li>
          ))}
        </ul>
        {live ? (
          <ArrowRightIcon className="size-4 shrink-0 text-gray-8 transition-[transform,color] duration-150 ease-out group-hover/card:translate-x-0.5 group-hover/card:text-gray-12" />
        ) : (
          <ArrowUpRightIcon className="size-4 shrink-0 text-gray-8 transition-[transform,color] duration-150 ease-out group-hover/card:translate-x-0.5 group-hover/card:-translate-y-0.5 group-hover/card:text-gray-12" />
        )}
      </div>
    </>
  )
  return live ? (
    <EventLink
      href={`/roadmap/${spec.slug}`}
      event="tech_spec_open"
      params={{ slug: spec.slug, cta_location: LOCATION }}
      className={card}
    >
      {inner}
    </EventLink>
  ) : (
    <a
      href={`${GITHUB_SPECS}/${spec.slug}`}
      target="_blank"
      rel="noopener noreferrer"
      aria-label={`${spec.title}, in draft on GitHub (opens in a new tab)`}
      className={card}
    >
      {inner}
    </a>
  )
}
