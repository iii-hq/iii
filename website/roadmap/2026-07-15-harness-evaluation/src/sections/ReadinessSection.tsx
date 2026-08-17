import { StepReveal } from '@lib/components/diagrams/StepReveal'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { READINESS_STAGES, READY_CONTRACT } from '../content/readiness'

export function ReadinessSection() {
  return (
    <Section
      id="readiness"
      index="05"
      eyebrow="track 1 / integration"
      title="control inference to prove the harness contract exactly."
      lede="the implemented integration runner owns fresh-stack readiness, six scripted router Functions, twelve explicit request matchers, exact generations, normative traces, and bounded process teardown."
    >
      <StepReveal title="event-driven harness bring-up" stages={READINESS_STAGES} />

      <div className="mt-6 grid grid-cols-1 gap-4 @4xl:grid-cols-2">
        <SpecSheet title="harness::ready wire contract" meta="strict config">
          {READY_CONTRACT.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
        </SpecSheet>

        <SpecSheet title="integration identity" meta="implemented in harness/tests/e2e">
          <SpecRow name="model boundary" type="scripted router">
            production llm-router and providers remain absent from the deterministic stack.
          </SpecRow>
          <SpecRow name="corpus" type="4 fixtures">
            E2E-001, E2E-002, UI-001, and UI-002 cover streaming, native calls, Console, and multiple traces.
          </SpecRow>
          <SpecRow name="oracle" type="exact public evidence">
            status, transcript, router history, target spans, lifecycle spans, process state, and teardown must agree.
          </SpecRow>
          <div className="mt-4">
            <a
              href="#/protocol"
              className="font-mono text-[12px] text-ink-faint lowercase transition-colors hover:text-accent"
            >
              inspect the integration protocol →
            </a>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
