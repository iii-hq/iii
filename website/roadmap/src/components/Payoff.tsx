import { cn } from "@lib/lib/utils"

export interface PayoffMetric {
  label: string
  before: string
  after: string
}

export interface PayoffRow {
  problem: string
  answer: string
  detail: string
}

/**
 * A11 — the before → after scorecard that opens a payoff slide. `valueClassName`
 * sets the size of the "after" figure (decks use 20px or 24px).
 */
export function PayoffScorecard({
  metrics,
  valueClassName = "text-[20px]",
  wrap = true,
}: {
  metrics: readonly PayoffMetric[]
  valueClassName?: string
  /** let a long before/after pair wrap onto two lines */
  wrap?: boolean
}) {
  return (
    <div className="grid grid-cols-2 @3xl:grid-cols-4 border-x border-t border-rule bg-rule gap-px">
      {metrics.map((m) => (
        <div key={m.label} className="bg-bg px-4 py-5 min-w-0">
          <div className={cn("flex items-baseline gap-x-2", wrap && "flex-wrap")}>
            <span className="font-mono text-[13px] text-ink-ghost line-through tabular-nums">{m.before}</span>
            <span className="font-mono text-[12px] text-ink-ghost">→</span>
            <span className={cn("font-mono font-semibold text-accent tabular-nums leading-none", valueClassName)}>
              {m.after}
            </span>
          </div>
          <div className="mt-2 font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint">{m.label}</div>
        </div>
      ))}
    </div>
  )
}

/** A11 — the problem → answer table: one red problem per row, its green answer and a line of detail beside it. */
export function PayoffTable({
  rows,
  problemHeading,
  answerHeading,
  answerClassName = "lowercase",
}: {
  rows: readonly PayoffRow[]
  problemHeading: string
  answerHeading: string
  /** the answer line keeps its own casing when the content is quoted verbatim */
  answerClassName?: string
}) {
  return (
    <div className="mt-8 grid grid-cols-1 @3xl:grid-cols-2 border border-rule bg-rule gap-px">
      <div className="bg-panel px-4 py-2.5 font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint">
        {problemHeading}
      </div>
      <div className="bg-panel px-4 py-2.5 font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint hidden @3xl:block">
        {answerHeading}
      </div>
      {rows.map((row) => (
        <div key={row.problem} className="contents">
          <div className="bg-bg px-4 py-4 min-w-0">
            <div className="font-mono text-[13px] text-alert lowercase">{row.problem}</div>
          </div>
          <div className="bg-bg px-4 py-4 min-w-0">
            <div className={cn("font-mono text-[13px] text-accent", answerClassName)}>{row.answer}</div>
            <div className="mt-1 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{row.detail}</div>
          </div>
        </div>
      ))}
    </div>
  )
}
