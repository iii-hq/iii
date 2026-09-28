import type { Metadata } from "next"
import type { CSSProperties } from "react"
import { GetStarted } from "@/components/landing/get-started"
import { InView } from "@/components/motion/in-view"
import { PageShell } from "@/components/site/page-shell"
import { keyed } from "@/lib/keys"
import { hero, type Inline, type Statement, statements } from "./manifesto-data"
import { Motif, Primitives } from "./motifs"

const description =
  "The iii manifesto. How software can be collapsed into three primitives: Worker, Trigger, Function. Linear scaling, zero integration cost, agent-native."
const ogTitle = "iii / manifesto — collapse the categories"

export const metadata: Metadata = {
  title: "iii / manifesto",
  description,
  keywords: [
    "iii",
    "manifesto",
    "three primitives",
    "worker",
    "trigger",
    "function",
    "distributed systems",
    "paradigm shift",
    "AI agents",
    "polyglot",
    "integration cost",
    "durable execution",
    "live discovery",
    "ontology",
  ],
  alternates: { canonical: "/manifesto" },
  openGraph: {
    type: "article",
    url: "/manifesto",
    title: ogTitle,
    description,
    images: [{ url: "/og-image.png", width: 1200, height: 630, type: "image/png" }],
  },
  twitter: {
    card: "summary_large_image",
    title: ogTitle,
    description:
      "Worker, Trigger, Function is the whole interface. Add a worker — the system absorbs it: live, discoverable, observable.",
    images: ["/og-image.png"],
  },
}

/** Explicit delay for a block in the opening cascade (see `.intro` in globals.css). */
const at = (ms: number) => ({ "--d": `${ms}ms` }) as CSSProperties

/** The lede under the headline: the manifesto's own summary, as the current site states it in its page description. */
const lede =
  "How software collapses into three primitives: Worker, Trigger, Function. Linear scaling, zero integration cost, agent-native."

/** Body copy with the source's italic phrases kept as `<i>`. */
function Body({ inline }: { inline: Inline[] }) {
  return keyed(inline, (run) => (typeof run === "string" ? run : `i:${run.i}`)).map(({ key, item: run }) =>
    typeof run === "string" ? (
      <span key={key}>{run}</span>
    ) : (
      <i key={key} className="text-gray-12">
        {run.i}
      </i>
    ),
  )
}

/**
 * One statement: its line drawing (drawn in as it arrives), the number in
 * mono, the title in one weight with the accent phrase in grey, the paragraph.
 * Left-aligned on the page grid, no box around it.
 */
function StatementBlock({ statement, index }: { statement: Statement; index: number }) {
  const titleId = `${statement.id}-title`
  return (
    <InView>
      <section id={statement.id} aria-labelledby={titleId} className="scroll-mt-[96px]">
        <div className="flex items-center gap-4">
          <Motif index={index} className="size-16 text-gray-12" />
          <span className="font-mono text-[12px] tracking-[0.04em] text-gray-9 tabular-nums">
            {String(index + 1).padStart(2, "0")}
          </span>
        </div>
        <h2
          id={titleId}
          className="mt-6 text-[clamp(22px,2.2vw,28px)] leading-[1.2] font-medium tracking-[-0.025em] text-balance text-gray-12"
        >
          {statement.code ? (
            <code className="font-mono text-[0.85em] font-medium tracking-[-0.01em] text-gray-10">
              {statement.accent}
            </code>
          ) : (
            <span className="text-gray-10">{statement.accent}</span>
          )}{" "}
          {statement.heavy}
          {statement.ghost && (
            <>
              {" "}
              <span className="text-gray-7">{statement.ghost}</span>
            </>
          )}
        </h2>
        <p className="mt-3 max-w-[52ch] text-[15px] leading-[1.65] text-pretty text-gray-11">
          <Body inline={statement.body} />
        </p>
      </section>
    </InView>
  )
}

export default function ManifestoPage() {
  return (
    <PageShell>
      {/* Head: the landing hero's grid. Copy on the leading edge, the drawing on the right. */}
      <section aria-labelledby="manifesto-title" className="pt-12 pb-16 sm:pt-20 sm:pb-24">
        <div className="mx-auto max-w-[1200px] px-5 md:px-6">
          {/* The first line is 38 characters, so the copy column runs wider than the landing hero's and the size
              is capped where that line still fits on one row (about 17.5em). */}
          <div className="grid grid-cols-[minmax(0,1fr)] items-center gap-12 lg:grid-cols-[minmax(0,8fr)_minmax(0,4fr)] lg:gap-16">
            <div className="max-w-[760px] min-w-0">
              <p style={at(40)} className="intro text-[13px] font-medium tracking-[0.02em] text-gray-10">
                Manifesto
              </p>
              {/* Two lines, as on the landing page: the problem, then the answer. */}
              {/* biome-ignore lint/correctness/useUniqueElementIds: one page heading; the section is labelled by it */}
              <h1
                id="manifesto-title"
                style={at(120)}
                className="intro mt-5 text-[clamp(28px,7.2vw,32px)] leading-[1.06] font-medium tracking-[-0.04em] text-gray-12 sm:text-[clamp(28px,3vw,42px)]"
              >
                <span className="sm:block">{hero.problem}</span>{" "}
                <span className="sm:block">
                  {hero.verb} {hero.answer}
                </span>
              </h1>
              <p
                style={at(240)}
                className="intro mt-5 max-w-[480px] text-[15px] leading-[1.65] text-pretty text-gray-11"
              >
                {lede}
              </p>
            </div>

            <InView className="intro-slow text-gray-8 max-lg:hidden" style={at(360)}>
              <Primitives className="h-auto w-full" />
            </InView>
          </div>
        </div>
      </section>

      {/* The twelve statements, two to a row, on the page grid. */}
      <article aria-label="The manifesto" className="pb-20 sm:pb-24">
        <div className="mx-auto max-w-[1200px] px-5 md:px-6">
          <div className="grid gap-x-16 gap-y-16 sm:gap-y-20 lg:grid-cols-2">
            {statements.map((statement, index) => (
              <StatementBlock key={statement.id} statement={statement} index={index} />
            ))}
          </div>
        </div>
      </article>

      <GetStarted />
    </PageShell>
  )
}
