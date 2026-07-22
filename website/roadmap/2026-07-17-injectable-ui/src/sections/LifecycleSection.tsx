import { StepReveal } from '@lib/components/diagrams/StepReveal'
import { Section } from '@lib/components/Section'
import { LIFECYCLE_STAGES } from '../content/lifecycle'

/**
 * A6 — the lifecycle guarantee: injected ui dies with its worker, survives
 * console and engine restarts, and a broken script never takes the shell down.
 */
export function LifecycleSection() {
  return (
    <Section
      id="lifecycle"
      index="08"
      eyebrow="lifecycle"
      title="injected ui dies with its worker."
      lede="sdk message-path registration only: the engine gcs a worker's triggers on disconnect, the sdk replays them on reconnect, the engine parks intents while the console is down. step through every failure the spec's lifecycle matrix covers."
    >
      <StepReveal title="the lifecycle matrix, walked" stages={LIFECYCLE_STAGES} />
    </Section>
  )
}
