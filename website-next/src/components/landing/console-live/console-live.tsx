import { SectionIntro } from "../section-intro"
import { ConsoleFrame } from "./console-frame"

/**
 * The recorded console session, framed as a product window. It plays while
 * it's on screen and pauses when it isn't. No pinning or scroll-driven zoom:
 * scrolling stays the reader's.
 */
export function ConsoleLive() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: the header's section rail targets this id
    <section id="console-live" aria-labelledby="console-live-title" className="scroll-mt-[116px] py-20 sm:py-24">
      <div className="mx-auto max-w-[1200px] px-5 md:px-6">
        <SectionIntro eyebrow="Agents" titleId="console-live-title" title="Same run, from inside the harness.">
          Your agentic harness is part of your system, so it runs faster, works better, and uses fewer tokens than any
          other harness.
        </SectionIntro>

        <ConsoleFrame className="mt-10 sm:mt-12" />
      </div>
    </section>
  )
}
