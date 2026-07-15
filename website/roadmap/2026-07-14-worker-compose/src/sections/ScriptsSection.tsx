import { StepReveal } from '@lib/components/diagrams/StepReveal'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { SCRIPT_FIELDS, SCRIPT_RULES, SCRIPT_STAGES } from '../content/scripts'

/**
 * A6 — the container lifecycle: prepare (blocking), supervise (the run),
 * clean up (fired after exit, not awaited). One supervised process, two
 * one-shot hooks.
 */
export function ScriptsSection() {
  return (
    <Section
      id="scripts"
      index="05"
      eyebrow="scripts"
      title="prepare. supervise. clean up."
      lede="three scripts per container, in the compose file: pre_start blocks with a 60s default budget, run is the process compose owns, post_run fires once the run exits — any exit path — and is never awaited."
    >
      <StepReveal title="one container through its lifetime" stages={SCRIPT_STAGES} />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="the fields" meta="scripts: per container">
          {SCRIPT_FIELDS.map((f) => (
            <SpecRow key={f.name} name={f.name}>
              {f.desc}
            </SpecRow>
          ))}
        </SpecSheet>
        <SpecSheet title="the hard lines" meta="validation + teardown">
          {SCRIPT_RULES.map((rule, i) => (
            <SpecRow key={i} name={`rule 0${i + 1}`}>
              {rule}
            </SpecRow>
          ))}
        </SpecSheet>
      </div>
    </Section>
  )
}
