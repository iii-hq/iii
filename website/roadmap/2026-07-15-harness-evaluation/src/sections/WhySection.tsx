import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { PAIN_CARDS, SPLIT_TERMS } from '../content/why'

/**
 * A2 — the problem. Name today's failures as concrete cards, then state the
 * split precisely: what the two tracks share and what they never share.
 */
export function WhySection() {
  return (
    <Section
      id="why"
      index="01"
      eyebrow="the problem"
      title="one test track lies to you, one way or the other."
      lede="the harness makes durable public promises (turns, streaming, dispatch, lifecycle) and separately promises that real workflows succeed. no single model boundary can check both honestly."
    >
      <div className="grid grid-cols-1 @2xl:grid-cols-2 gap-px bg-rule border border-rule">
        {PAIN_CARDS.map((card) => (
          <div key={card.n} className="bg-bg p-6 @3xl:p-7 min-w-0">
            <div className="flex items-baseline gap-x-2.5 mb-3">
              <span className="font-mono text-[11px] text-alert tabular-nums">{card.n}</span>
              <span className="font-mono text-[15px] font-semibold text-ink lowercase">{card.title}</span>
            </div>
            <p className="font-mono text-[13px] leading-[1.7] text-ink-faint lowercase max-w-[46ch]">{card.body}</p>
          </div>
        ))}
      </div>

      <div className="mt-8">
        <SpecSheet title="the split, stated precisely" meta="the load-bearing constraint">
          <div className="flex flex-col">
            {SPLIT_TERMS.map((row) => (
              <SpecRow key={row.name} name={row.name} type={row.type}>
                {row.desc}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
