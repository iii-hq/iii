import { Section } from '@/components/Section'
import { CliPlayground } from '@/components/diagrams/CliPlayground'
import { StatusPanel } from '@/components/schematic/StatusPanel'
import { TRACKS } from '@/content/playground'

export function PlaygroundSection() {
  return (
    <Section
      id="playground"
      index="02"
      eyebrow="run it"
      title="a simulated terminal running real iii commands."
      lede="step through the golden path, then day-2 ops. each command shows its backing function + owner — because in the new cli, every command is an iii function."
    >
      <CliPlayground tracks={TRACKS} />

      <div className="mt-10">
        <StatusPanel
          variant="info"
          headline="the iii up step is where the planes meet"
          detail="ops → daemon → sandbox → connect-back. see the sequence in 'the three meeting points' below."
        />
      </div>

      <div className="mt-6 flex gap-3">
        <a
          href="#meeting-points"
          className="inline-flex items-center border border-rule bg-bg px-3 py-2 font-mono text-[13px] lowercase text-ink hover:bg-panel transition-colors"
        >
          see the three meeting points →
        </a>
        <a
          href="#/playground"
          className="inline-flex items-center border border-rule bg-bg px-3 py-2 font-mono text-[13px] lowercase text-ink hover:bg-panel transition-colors"
        >
          open the full playground →
        </a>
      </div>
    </Section>
  )
}
