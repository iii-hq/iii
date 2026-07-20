import { StepReveal } from '@lib/components/diagrams/StepReveal'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { WIRE_STAGES } from '../content/wire'

/**
 * A6 — the wire contract: what one registration does inside the console's
 * TriggerHandler, and why it must be serialized.
 */
export function WireSection() {
  return (
    <Section
      id="wire"
      index="04"
      eyebrow="the wire contract"
      title="same path ⇒ override, serialized."
      lede="trigger identity is a uuid the registrant's sdk mints; path identity is console policy. the handler drains every register/unregister event through one mpsc queue in ws-arrival order — the content fetch inside the serialized section."
    >
      <StepReveal title="one registration through the handler" stages={WIRE_STAGES} />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="why the queue is load-bearing" meta="found in review">
          <div className="flex flex-col">
            <SpecRow name="the race" type="unserialized">
              the sdk spawns each trigger callback on its own task. two rapid re-registrations of one path can both
              observe the same old id, both unregister it, and whichever fetch finishes last wins — possibly the
              older build, plus a stale engine row.
            </SpecRow>
            <SpecRow name="the fix" type="one consumer">
              all script/style events — including the engine&apos;s unregister echo — drain through a single mpsc
              queue in arrival order. the echo lands after the commit it echoes and hits the unknown-id no-op branch.
            </SpecRow>
            <SpecRow name="no deadlock" type="verified">
              holding the section across engine::unregister_trigger cannot deadlock — the engine only awaits
              enqueueing the echo into the console&apos;s outbound channel.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="acks and rejection" meta="two registration paths">
          <div className="flex flex-col">
            <SpecRow name="sdk message path" type="recommended">
              fire-and-forget; a console Err takes the engine&apos;s late-unwind — the trigger is removed and the
              TriggerRegistrationResult reaches the registrant.
            </SpecRow>
            <SpecRow name="engine::register_trigger" type="avoid">
              durable triggers outlive their worker (a page pointing at a dead content function) and vanish on
              engine restart with no replayer. ack degrades to fail-open past the 10s window.
            </SpecRow>
            <SpecRow name="parked intents" type="v1 accepted">
              invisible to registered-triggers::list and produce no deferred ack; the asset appears when the console
              does.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
