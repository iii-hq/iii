import { CliPlayground } from '@/components/diagrams/CliPlayground'
import { Section } from '@/components/Section'
import { CLI_TRACKS } from '@/content/example'

/**
 * A3 — the CLI playground. Show it, don't tell it: a simulated terminal the
 * reader can step through. Two tracks here — first run and day-2 ops.
 */
export function InstallSection() {
  return (
    <Section
      id="install"
      index="06"
      eyebrow="run it yourself"
      title="from zero to running, in three commands."
      lede="press play, or step through it. switch tracks to see day-2 operations. every command maps to a real function on the bus."
    >
      <CliPlayground title="simulated terminal" tracks={CLI_TRACKS} />
    </Section>
  )
}
