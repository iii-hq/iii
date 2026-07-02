import { FanOut } from '@lib/components/diagrams/FanOut'
import { Section } from '@lib/components/Section'
import { FAN_HANDLERS, FAN_SOURCE, FAN_TRIGGER } from '../content/harness'

/**
 * A7 - fan-out. The harness payoff: it emits an event when a turn finishes, and
 * codegen turns that into a typed on-handler, so every consumer that reacts is
 * typed end to end. Links to the full consumer walkthrough.
 */
export function HarnessSection() {
  return (
    <Section
      id="harness"
      index="07"
      eyebrow="leveraged by harness"
      title="bind the harness loop with a typed handler."
      lede="harness emits harness::turn-completed when a turn reaches a terminal status, carrying the result. codegen generates harness.onTurnCompleted(config, handler): a typed config, a typed payload, a typed return, so every consumer that reacts is checked at compile time."
    >
      <FanOut
        title="one event, every typed consumer"
        source={FAN_SOURCE}
        trigger={FAN_TRIGGER}
        handlers={FAN_HANDLERS}
      />

      <div className="mt-6 flex flex-col gap-3 @3xl:flex-row @3xl:items-center @3xl:justify-between border border-rule bg-panel px-5 py-4">
        <p className="font-mono text-[12px] leading-[1.6] text-ink-faint lowercase max-w-[64ch]">
          harness exposes typed functions (harness::send) and seven trigger types: two async events and five sync hooks.
          point codegen at it once and the whole surface becomes a typed client.
        </p>
        <a
          href="#/harness-consumer"
          className="inline-flex h-10 items-center bg-ink text-bg border border-ink px-4 font-mono text-[13px] lowercase whitespace-nowrap transition-colors hover:bg-bg hover:text-ink focus-visible:ring-1 focus-visible:ring-accent focus:outline-none"
        >
          build a typed consumer →
        </a>
      </div>
    </Section>
  )
}
