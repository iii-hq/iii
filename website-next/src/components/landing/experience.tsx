import { type CodeLine, tokenize } from "@/lib/shiki"
import { Sequencer } from "./experience/sequencer"
import { HOMEPAGE_FLOW } from "./experience/steps"
import { SectionIntro } from "./section-intro"

/**
 * "Any task, one experience": the scripted work session, played in a Slack
 * window. Server component: every code step's file is highlighted here with
 * Shiki and handed to the client as token data, which then reveals the lines.
 */
export async function Experience() {
  const code: Record<string, CodeLine[]> = {}
  await Promise.all(
    HOMEPAGE_FLOW.map(async (step) => {
      if (step.type !== "code-editor") return
      code[step.id] = await tokenize(step.lines.join("\n"), step.language)
    }),
  )

  return (
    // biome-ignore lint/correctness/useUniqueElementIds: the header's section rail targets this id
    <section id="experience" aria-labelledby="experience-title" className="scroll-mt-[116px] py-20 sm:py-24">
      <div className="mx-auto max-w-[1200px] px-5 md:px-6">
        <SectionIntro eyebrow="Why iii" titleId="experience-title" title="Any task, one experience.">
          iii makes it unreasonably efficient to create and extend software. Follow one request from a Slack message to
          running services. It pauses for you at Send and Deploy, and carries on by itself if you just watch.
        </SectionIntro>

        <Sequencer code={code} className="mt-10 sm:mt-12" />
      </div>
    </section>
  )
}
