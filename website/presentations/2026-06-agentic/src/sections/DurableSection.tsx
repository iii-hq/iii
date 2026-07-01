import { DurabilityTimeline } from '@lib/components/diagrams/DurabilityTimeline'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { DURABILITY_STAGES } from '../content/durability'

export function DurableSection() {
  return (
    <Section
      id="durable"
      index="06"
      eyebrow="durable execution"
      title="a turn is state, not a process — it can crash, park for a week, and resume."
      lede="the loop runs as enqueued, replay-safe steps. that one decision buys everything below: crash recovery without duplicates, long-lived holds that cost nothing while they wait, and sub-agents that join whenever they finish."
    >
      <DurabilityTimeline
        stages={DURABILITY_STAGES}
        heading="one turn vs. a crash and a long wait"
        headingNote="turn t_001"
        recordHeading="the turn record — state, not a process"
      />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="the redelivery rules" meta="at-least-once, tamed">
          <div className="flex flex-col">
            <SpecRow name="stale-step guard" type="per dequeue">
              every step compares its number against the turn record; anything older than the current step is
              acknowledged and dropped.
            </SpecRow>
            <SpecRow name="deterministic entry ids" type="idempotent writes">
              e_&lt;turn&gt;_&lt;step&gt;_assistant, e_&lt;turn&gt;_&lt;call_id&gt; — a replayed step writes into the
              same entries instead of duplicating them.
            </SpecRow>
            <SpecRow name="per-call checkpoints" type="dispatched → pending → done">
              a call caught between dispatched and done is never re-invoked; the model gets an explicit "result unknown"
              and chooses.
            </SpecRow>
            <SpecRow name="net effect" type="invariant">
              step delivery is at-least-once; function side effects are at-most-once; transcripts never fork from a
              crash.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="parked turns — the long-wait primitive" meta="hold / resume">
          <div className="flex flex-col">
            <SpecRow name="why park" type="design">
              an approval that waits for a human, a child that runs for an hour — holding a queue step open would couple
              wait time to infrastructure timeouts. parking decouples them.
            </SpecRow>
            <SpecRow name="what parks" type="mechanics">
              the dispatch reports pending, the step ends cleanly, and the turn waits as a record in state — steering
              messages still fold in while it sleeps.
            </SpecRow>
            <SpecRow name="what wakes it" type="harness::function::resolve">
              a human decision, a child completing, or the timeout sweep — whichever lands first; duplicates are no-ops
              by construction.
            </SpecRow>
            <SpecRow name="nothing parks forever" type="backstop">
              every pending call carries a timeout; a periodic sweep settles expired calls with an explicit error the
              model can react to.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
