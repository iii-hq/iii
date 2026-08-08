import { SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { Section } from '@lib/components/Section'
import { ORDER_LANES, ORDER_STEPS } from '../content/ordering'

/**
 * A5 — the optimistic registration buffer: a route that arrives before its
 * router waits in the engine instead of being dropped.
 */
export function OrderingSection() {
  return (
    <Section
      id="ordering"
      index="08"
      eyebrow="ordering"
      title="order becomes performance, not correctness."
      lede="today a registration that arrives before its trigger owner is a warning and a dropped route. the engine gains a bounded buffer; compose exposes the timeout and retry knobs."
    >
      <SequencePlayer title="register early · buffer · flush" lanes={ORDER_LANES} steps={ORDER_STEPS} />
    </Section>
  )
}
