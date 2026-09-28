import { type CodeLang, tokenize } from "@/lib/shiki"
import { RaceStage } from "./harness-race/race-stage"
import { III, type Line, TRAD } from "./harness-race/script"
import type { TokenMap } from "./harness-race/timeline"
import { SectionIntro } from "./section-intro"

type CodeLineDef = Line & { t: string; lang: CodeLang }
const isCode = (l: Line): l is CodeLineDef => Boolean(l.lang) && typeof l.t === "string"

/** Shiki tokens for every code line in the script, keyed by text. Runs once per render on the server. */
async function tokenizeScript(): Promise<TokenMap> {
  const lines = [...III, ...TRAD].flat().filter(isCode)
  const entries = await Promise.all(lines.map(async (l) => [l.t, (await tokenize(l.t, l.lang))[0] ?? []] as const))
  return Object.fromEntries(entries)
}

// HARNESS. The split race: a traditional coding agent (left) against the same
// prompt run through iii (right). The console section right above
// (ConsoleLive) shows the same run from inside the harness.
export async function HarnessRace() {
  const code = await tokenizeScript()
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: the header's section rail and #harness anchors target this id
    <section id="harness" aria-labelledby="harness-title" className="scroll-mt-[116px] py-20 sm:py-24">
      <div className="mx-auto max-w-[1200px] px-5 md:px-6">
        <SectionIntro eyebrow="Harness" titleId="harness-title" title="Same work, two outcomes.">
          One agent researches a stack and resolves merge conflicts. The other finds the workers it needs and ships in
          the same time, for a fraction of the tokens.
        </SectionIntro>
        <RaceStage code={code} className="mt-10 sm:mt-12" />
      </div>
    </section>
  )
}
