import { StepReveal } from '@/components/diagrams/StepReveal'
import { Section } from '@/components/Section'
import { REVEAL_STAGES } from '@/content/example'

/**
 * A6 — a lifecycle / durability walker. Step through a process whose state
 * evolves; here, one request surviving a crash. Use Step Reveal for boot
 * sequences, durability timelines, readiness bring-up.
 */
export function LifecycleSection() {
  return (
    <Section
      id="lifecycle"
      index="03"
      eyebrow="durability"
      title="a crash is a state transition, not lost work."
      lede="step through one request as a worker dies mid-flight. the committed log survives; a fresh worker resumes from the last step. nothing replays twice."
    >
      <StepReveal title="one request, surviving a crash" stages={REVEAL_STAGES} />
    </Section>
  )
}
