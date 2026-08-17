import { SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { RUN_LANES, RUN_STEPS } from '../content/run-sequence'

export function RunSequenceSection() {
  return (
    <Section
      id="lifecycle"
      index="04"
      eyebrow="shared lifecycle"
      title="the run shape is shared until the model boundary and shared again at evidence."
      lede="step through the complete flow. the tracks differ at subject identity, inference, and grading policy; setup, harness::send, durable completion, evidence completeness, classification, and cleanup stay structurally aligned."
    >
      <SequencePlayer
        title="select → pin → boot → send → infer → collect → grade"
        lanes={RUN_LANES}
        steps={RUN_STEPS}
      />

      <div className="mt-6 grid grid-cols-1 gap-4 @4xl:grid-cols-2">
        <SpecSheet title="integration owns" meta="exact contracts">
          <SpecRow name="router requests" type="12 fields">
            each generation matches the complete normalized request before frames stream.
          </SpecRow>
          <SpecRow name="target + lifecycle" type="exact traces">
            controlled calls and completion delivery must have the expected identity and count.
          </SpecRow>
          <SpecRow name="processes" type="fresh stack">
            every selected fixture owns isolated stores, process groups, and bounded teardown.
          </SpecRow>
        </SpecSheet>

        <SpecSheet title="quality owns" meta="real-model outcome">
          <SpecRow name="subject" type="pinned identity">
            one model, provider, prompt, skill set, and artifact set applies to the complete run.
          </SpecRow>
          <SpecRow name="domain checks" type="scenario-specific">
            durable external effects prove correctness independently from the final assistant message.
          </SpecRow>
          <SpecRow name="budgets" type="reported + enforced">
            time, tokens, cost, turns, calls, and errors stay separate dimensions.
          </SpecRow>
        </SpecSheet>
      </div>
    </Section>
  )
}
