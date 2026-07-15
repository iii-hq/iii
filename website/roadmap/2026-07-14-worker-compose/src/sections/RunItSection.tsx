import { CliPlayground } from '@lib/components/diagrams/CliPlayground'
import { Section } from '@lib/components/Section'
import { CLI_TRACKS } from '../content/cli-tracks'

/**
 * A3 — the demo, early. Four tracks: the golden path, the idempotent repeat,
 * the collision that fails loud, and the ordered teardown.
 */
export function RunItSection() {
  return (
    <Section
      id="run-it"
      index="02"
      eyebrow="run it"
      title="the whole stack, one command."
      lede="up starts the graph in dependency order and answers with a structured result. run it again: a no-op. run it from a second daemon: a rejection, not a zombie."
    >
      <CliPlayground tracks={CLI_TRACKS} title="worker-compose.yaml · database ← api" />
    </Section>
  )
}
