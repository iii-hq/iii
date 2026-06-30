import { FanOut } from '@/components/diagrams/FanOut'
import { Section } from '@/components/Section'
import { FAN_HANDLERS, FAN_SOURCE, FAN_TRIGGER } from '@/content/example'

/**
 * A7 — reactive fan-out. One write emits an event that fans to every bound
 * handler; no explicit publish step. Use when the spec has a trigger/event
 * model.
 */
export function ReactiveSection() {
  return (
    <Section
      id="reactive"
      index="04"
      eyebrow="reactive by design"
      title="one write. every surface, live."
      lede="every change to the store emits an event. bind a handler to it once and render live — no polling, no status chasing, no manual publish."
    >
      <FanOut
        title="one write, every surface"
        source={FAN_SOURCE}
        trigger={FAN_TRIGGER}
        handlers={FAN_HANDLERS}
      />
    </Section>
  )
}
