import { CliPlayground } from '@lib/components/diagrams/CliPlayground'
import { Section } from '@lib/components/Section'
import { CLI_TRACKS } from '../content/cli'

/**
 * A3 - run it. The interactive proof, early: one command generates a typed
 * client; the second track shows --check guarding drift in ci. Step through
 * the transcript or switch tracks.
 */
export function RunItSection() {
  return (
    <Section
      id="run"
      index="02"
      eyebrow="run it"
      title="one command. a typed client in your repo."
      lede="codegen connects to the engine, reads the catalog, and writes the files your codegen.yml asked for. in ci, --check regenerates in memory and fails if anything would change. drift is caught before it ships."
    >
      <CliPlayground title="codegen, run two ways" tracks={CLI_TRACKS} />
    </Section>
  )
}
