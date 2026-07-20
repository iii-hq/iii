import { Section } from '@lib/components/Section'
import { BreakTheRun } from '../diagrams/BreakTheRun'

/**
 * Deck-local interactive — the reader plays the misbehaving party and the
 * evidence catches it. The claim: every failure gets named, classified, and
 * blamed on the right party; honest play is the only path to exit 0.
 */
export function BreakSection() {
  return (
    <Section
      id="break"
      index="08"
      eyebrow="integration · play it"
      title="try to sneak one past the oracles."
      lede="you are the misbehaving model, the flaky stack, the sloppy fixture author. pick a sabotage and watch the evidence catch it, classify it, and blame the right party. all six classifications are in there."
    >
      <BreakTheRun />
    </Section>
  )
}
