import type { CSSProperties } from "react"
import { Words } from "@/components/motion/words"
import { ArrowRightIcon } from "@/components/site/iconly"
import { SiteLink } from "@/components/site/site-link"
import { links } from "@/lib/site"
import { GlyphField } from "./glyph-field"
import { HeroActions } from "./hero-actions"
import { StageWindow } from "./stage-window"

/** Explicit delay for a block in the opening cascade (see `.intro` in globals.css). */
const at = (ms: number) => ({ "--d": `${ms}ms` }) as CSSProperties

export function Hero() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: fixed id; the header reveals its section rail after it
    <section id="hero" aria-label="Hero" className="relative isolate overflow-hidden pt-28 pb-16 sm:pt-32 sm:pb-24">
      {/* Decorative texture: the whole trailing half of the viewport, behind the copy.
          Hidden below lg, where it would only push the copy down. */}
      <div
        aria-hidden="true"
        style={at(400)}
        className="intro-slow pointer-events-none absolute top-0 right-0 -z-10 hidden h-[760px] w-1/2 select-none lg:block [mask-composite:intersect] [mask-image:linear-gradient(to_right,transparent,black_28%),linear-gradient(to_bottom,transparent,black_14%,black_55%,transparent)]"
      >
        <GlyphField className="h-full w-full" />
      </div>

      <div className="mx-auto max-w-[1200px] px-5 md:px-6">
        <div className="relative grid grid-cols-[minmax(0,1fr)] items-center gap-10 lg:min-h-[520px] lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)]">
          {/* Copy column, on the leading edge. `data-llms` marks the prose scripts/generate-llms-agents.ts extracts. */}
          <div
            data-llms="hero"
            className="relative z-10 flex w-full max-w-[600px] min-w-0 flex-col items-start text-left"
          >
            <SiteLink
              href={links.manifesto}
              style={at(40)}
              className="intro group/eyebrow inline-flex h-8 items-center gap-2 rounded-full bg-gray-2 pr-3 pl-1.5 text-[13px] whitespace-nowrap text-gray-11 shadow-[inset_0_0_0_1px_var(--gray-6)] transition-[box-shadow,color] duration-150 ease-[ease] hover:text-gray-12 hover:shadow-[inset_0_0_0_1px_var(--gray-8)] max-sm:pl-3 max-sm:text-[12px]"
            >
              <span className="rounded-full bg-gray-12 px-2 py-0.5 text-[11px] font-medium text-gray-1 max-sm:hidden">
                Open source
              </span>
              Three primitives. Zero integration cost.
              <ArrowRightIcon className="size-3.5 text-gray-9 transition-transform duration-200 ease-out group-hover/eyebrow:translate-x-0.5" />
            </SiteLink>

            {/* Words carry their own intro; the h1 itself doesn't animate, so nothing double-fades. */}
            <h1 className="mt-7 text-[clamp(24px,7.7vw,31px)] leading-[1.08] font-medium tracking-[-0.04em] text-gray-12 sm:text-[clamp(30px,4.6vw,52px)] sm:leading-[1.06]">
              {/* One sentence per line at every width: the size is derived from the first sentence's
                  width (11.25em) against the column, so neither line ever wraps. */}
              <span className="block">
                <Words text="Stop integrating services." from={0} />
              </span>
              <span className="block">
                <Words text="Start adding them." from={3} />
              </span>
            </h1>

            <p style={at(560)} className="intro mt-5 max-w-[480px] text-[14px] leading-[1.65] text-pretty text-gray-11">
              iii is one open-source engine. Every service and AI agent plugs in as a worker, in any language, with no
              glue code to write.
            </p>

            <div style={at(680)} className="intro mt-9 w-full">
              <HeroActions />
            </div>
          </div>
        </div>
      </div>

      <div style={at(520)} className="intro-slow mx-auto mt-16 max-w-[1200px] px-5 sm:mt-20 md:px-6">
        <StageWindow />
      </div>
    </section>
  )
}
