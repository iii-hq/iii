import { Section } from '@/components/Section'
import { PAYOFF_METRICS, PAYOFF_SOLVES } from '@/content/example'

/**
 * A11 — the payoff. Closes the persuasion loop: a quantified before/after
 * scorecard, then a problem → answer table. The reader should leave convinced
 * the work is worth doing.
 */
export function PayoffSection() {
  return (
    <Section
      id="payoff"
      index="07"
      eyebrow="the payoff"
      title="smaller surface, stronger guarantees."
      lede="the same system, measured before and after. the failure modes that used to need discipline are now gone by construction."
    >
      {/* scorecard */}
      <div className="grid grid-cols-2 @3xl:grid-cols-4 border-x border-t border-rule bg-rule gap-px">
        {PAYOFF_METRICS.map((m) => (
          <div key={m.label} className="bg-bg px-4 py-5 min-w-0">
            <div className="flex items-baseline gap-x-2">
              <span className="font-mono text-[13px] text-ink-ghost line-through tabular-nums">
                {m.before}
              </span>
              <span className="font-mono text-[12px] text-ink-ghost">→</span>
              <span className="font-mono text-[24px] font-semibold text-accent tabular-nums leading-none">
                {m.after}
              </span>
            </div>
            <div className="mt-2 font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint">
              {m.label}
            </div>
          </div>
        ))}
      </div>

      {/* problem → answer */}
      <div className="mt-8 grid grid-cols-1 @3xl:grid-cols-2 border border-rule bg-rule gap-px">
        <div className="bg-panel px-4 py-2.5 font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint">
          the problem
        </div>
        <div className="bg-panel px-4 py-2.5 font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint hidden @3xl:block">
          the answer
        </div>
        {PAYOFF_SOLVES.map((row) => (
          <div key={row.problem} className="contents">
            <div className="bg-bg px-4 py-4 min-w-0">
              <div className="font-mono text-[13px] text-alert lowercase">
                {row.problem}
              </div>
            </div>
            <div className="bg-bg px-4 py-4 min-w-0">
              <div className="font-mono text-[13px] text-accent lowercase">
                {row.answer}
              </div>
              <div className="mt-1 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">
                {row.detail}
              </div>
            </div>
          </div>
        ))}
      </div>
    </Section>
  )
}
