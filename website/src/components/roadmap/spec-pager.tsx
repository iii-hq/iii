import { EventLink } from "@/components/landing/roadmap-preview/event-link"
import { ArrowRightIcon } from "@/components/site/iconly"
import type { Spec } from "@/lib/specs"
import { cn } from "@/lib/utils"

const LOCATION = "tech_spec_pager"

const CELL = cn(
  "group/pager -mx-4 flex min-h-[44px] flex-col gap-1 rounded-[12px] px-4 py-3 outline-none",
  "transition-[background-color] duration-150 ease-out hover:bg-gray-3",
  "focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-1",
)

function PagerLink({ spec, direction }: { spec: Spec; direction: "newer" | "older" }) {
  const older = direction === "older"
  return (
    <EventLink
      href={`/roadmap/${spec.slug}`}
      event="tech_spec_open"
      params={{ slug: spec.slug, cta_location: LOCATION }}
      className={cn(CELL, older ? "items-end text-right sm:col-start-2" : "items-start")}
    >
      <span className="flex items-center gap-1.5 text-[13px] text-gray-10">
        {!older && (
          <ArrowRightIcon className="size-3.5 rotate-180 transition-transform duration-150 ease-out group-hover/pager:-translate-x-0.5" />
        )}
        {older ? "Older" : "Newer"}
        {older && (
          <ArrowRightIcon className="size-3.5 transition-transform duration-150 ease-out group-hover/pager:translate-x-0.5" />
        )}
      </span>
      <span className="text-[15px] leading-[1.4] font-medium tracking-[-0.01em] text-gray-12">{spec.title}</span>
    </EventLink>
  )
}

/** Newer / older spec, the list being newest first. */
export function SpecPager({ newer, older, className }: { newer?: Spec; older?: Spec; className?: string }) {
  if (!newer && !older) return null
  return (
    <nav aria-label="Other specs" className={cn("grid gap-3 sm:grid-cols-2", className)}>
      {newer && <PagerLink spec={newer} direction="newer" />}
      {older && <PagerLink spec={older} direction="older" />}
    </nav>
  )
}
