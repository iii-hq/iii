import { CliPlayground } from '@lib/components/diagrams/CliPlayground'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { CLI_TRACKS } from '../content/cli'

export function RunItSection() {
  return (
    <Section
      id="run"
      index="02"
      eyebrow="operate"
      title="select the track before selecting the scenario."
      lede="switch between the checked-in integration flow and the quality launcher contract. integration pins scripted generations; quality pins the complete run subject and preserves real-model variation in the report."
    >
      <CliPlayground tracks={CLI_TRACKS} title="evaluation operator flow" />

      <div className="mt-6 grid grid-cols-1 gap-4 @4xl:grid-cols-2">
        <SpecSheet title="integration selection" meta="deterministic">
          <SpecRow name="validate all" type="4 fixtures">
            compiles direct and playground fixtures without booting.
          </SpecRow>
          <SpecRow name="run all" type="2 direct">
            executes E2E-001 and E2E-002 serially on fresh stacks.
          </SpecRow>
          <SpecRow name="playground one" type="Console / Playwright">
            boots UI-001 or UI-002 and hands external stimulus to the driver.
          </SpecRow>
        </SpecSheet>

        <SpecSheet title="quality selection" meta="one subject per run">
          <SpecRow name="subject" type="launcher supplied">
            model, provider, prompt, skills, harness, workers, and options are resolved before the corpus.
          </SpecRow>
          <SpecRow name="corpus" type="5 scenarios">
            the same scenario source runs unchanged against another pinned subject.
          </SpecRow>
          <SpecRow name="comparison" type="independent reports">
            v1 supports manual baseline/candidate review without paired statistics or one aggregate score.
          </SpecRow>
        </SpecSheet>
      </div>
    </Section>
  )
}
