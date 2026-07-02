import { Section } from '@lib/components/Section'
import { Cell } from '@lib/components/schematic/Cell'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { WHY_CARDS } from '../content/why'

/**
 * A2 - the blind spot. Names today's failures as concrete cards so the reader
 * feels the tangle before the fix. Every card is a real cost of calling a
 * worker by hand.
 */
export function WhySection() {
  return (
    <Section
      id="why"
      index="01"
      eyebrow="the blind spot"
      title="calling another worker feels typed. it isn't."
      lede="the sdk hands you trigger<TInput, TOutput>, so the call looks safe. but you supply those types by hand, tied to nothing the target worker actually registered. here is what that costs."
    >
      <div className="grid grid-cols-1 @2xl:grid-cols-2 @4xl:grid-cols-3 gap-px bg-rule border border-rule">
        {WHY_CARDS.map((card) => (
          <Cell
            key={card.title}
            className="border-0"
            bodyClassName="max-w-[42ch]"
            title={
              <span className="flex items-center gap-x-2.5">
                <StatusDot tone="alert" />
                <span className="text-ink">{card.title}</span>
              </span>
            }
          >
            {card.body}
          </Cell>
        ))}
      </div>
    </Section>
  )
}
