import { type CodeLine, tokenize } from "@/lib/shiki"
import { type CommentKey, WORKER_ORDER, WORKERS, type WorkerId } from "./hello-world/flow-data"
import { HelloFlow } from "./hello-world/hello-flow"
import { SectionIntro } from "./section-intro"

export type WorkerCode = Record<WorkerId, CodeLine[]>
export type CommentTokens = Partial<Record<CommentKey, CodeLine>>

/**
 * "Any language, one protocol." Node orchestrates a Rust and a Python worker,
 * step by step. The three snippets (and the result comments the flow reveals)
 * are highlighted here on the server; the client only moves highlights around.
 */
export async function HelloWorld() {
  const [lines, commentLines] = await Promise.all([
    Promise.all(WORKER_ORDER.map((id) => tokenize(WORKERS[id].code, WORKERS[id].lang))),
    Promise.all(
      (WORKERS.ts.comments ?? []).map(async (c) => [c.key, (await tokenize(c.text, "typescript"))[0]] as const),
    ),
  ])
  const code = Object.fromEntries(WORKER_ORDER.map((id, i) => [id, lines[i]])) as WorkerCode
  const comments = Object.fromEntries(commentLines) as CommentTokens

  return (
    // biome-ignore lint/correctness/useUniqueElementIds: the header's section rail targets this id
    <section id="hello" aria-labelledby="hello-title" className="scroll-mt-[116px] py-20 sm:py-24">
      <div className="mx-auto max-w-[1200px] px-5 md:px-6">
        <SectionIntro eyebrow="Languages" titleId="hello-title" title="Any language, one protocol.">
          Python registers a function. Rust registers a function. Node consumes both.
        </SectionIntro>

        <HelloFlow code={code} comments={comments} className="mt-10 sm:mt-12" />
      </div>
    </section>
  )
}
