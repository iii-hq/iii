import { Section } from '@lib/components/Section'
import { Cell } from '@lib/components/schematic/Cell'
import { PAYOFF_TRADEOFFS_TITLE, SCORECARD, TRADEOFFS } from '../content/payoff'

/**
 * A11 — the payoff: every problem from the why section answered, plus the
 * honest costs. Admitted limits read as credibility.
 */
export function PayoffSection() {
  return (
    <Section
      id="payoff"
      index="10"
      eyebrow="payoff"
      title="smaller surface, stronger guarantees."
      lede="every failure from the top of this deck, answered by a mechanism you just walked through — and the three costs this pack pays for them, stated plainly."
    >
      <div className="border border-rule bg-bg flex flex-col">
        {SCORECARD.map((row) => (
          <div
            key={row.problem}
            className="grid grid-cols-1 @3xl:grid-cols-2 gap-x-6 gap-y-1 px-4 py-3.5 border-b border-rule-2 last:border-b-0"
          >
            <span className="font-mono text-[13px] text-ink-faint lowercase">{row.problem}</span>
            <span className="font-mono text-[13px] text-ink lowercase">{row.answer}</span>
          </div>
        ))}
      </div>

      <div className="mt-8">
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          {PAYOFF_TRADEOFFS_TITLE}
        </div>
        <div className="grid grid-cols-1 @4xl:grid-cols-3 gap-px bg-rule border border-rule">
          {TRADEOFFS.map((item) => (
            <Cell key={item.title} title={item.title} className="border-0">
              {item.body}
            </Cell>
          ))}
        </div>
      </div>
    </Section>
  )
}
