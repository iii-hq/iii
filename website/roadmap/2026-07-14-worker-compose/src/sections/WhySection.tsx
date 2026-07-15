import { Section } from '@lib/components/Section'
import { Cell } from '@lib/components/schematic/Cell'
import { WHY_CARDS } from '../content/why'

/**
 * A2 — the pain. Six concrete failures in today's lifecycle, each cited to
 * code or to the review that surfaced it.
 */
export function WhySection() {
  return (
    <Section
      id="why"
      index="01"
      eyebrow="why"
      title="today: six ways processes leak."
      lede="none of these are hypothetical — each one is in the code or in last week's review. compose exists because a supervisor cannot be bolted onto contracts that do not exist."
    >
      <div className="grid grid-cols-1 @2xl:grid-cols-2 @5xl:grid-cols-3 gap-px bg-rule border border-rule">
        {WHY_CARDS.map((card, i) => (
          <Cell
            key={card.title}
            title={
              <span className="flex items-center gap-x-2.5">
                <span className="font-mono text-[11px] text-ink-ghost tabular-nums">0{i + 1}</span>
                {card.title}
              </span>
            }
            className="border-0"
          >
            <p>{card.body}</p>
            <p className="mt-3 font-mono text-[10px] uppercase tracking-[0.06em] text-ink-ghost">{card.cite}</p>
          </Cell>
        ))}
      </div>
    </Section>
  )
}
