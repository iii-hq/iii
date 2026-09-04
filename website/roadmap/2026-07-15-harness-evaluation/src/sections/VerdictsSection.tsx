import { StepReveal } from '@lib/components/diagrams/StepReveal'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { INTEGRATION_REPORTS, QUALITY_REPORTS, VERDICT_STAGES } from '../content/verdicts'

export function VerdictsSection() {
  return (
    <Section
      id="verdicts"
      index="09"
      eyebrow="failure semantics"
      title="a red run must still say which layer failed."
      lede="the exact labels differ by track, but the ordering principle is shared: incomplete evidence, infrastructure, timeout, and cleanup remain first-class outcomes instead of being flattened into an assertion or passing skip."
    >
      <StepReveal title="shared failure ladder · low → high" stages={VERDICT_STAGES} />

      <div className="mt-6 grid grid-cols-1 gap-4 @4xl:grid-cols-2">
        <SpecSheet title="integration report set" meta="deterministic contract">
          {INTEGRATION_REPORTS.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
        </SpecSheet>

        <SpecSheet title="quality report set" meta="correctness + efficiency">
          {QUALITY_REPORTS.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
        </SpecSheet>
      </div>
    </Section>
  )
}
