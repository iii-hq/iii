import { StepReveal } from '@lib/components/diagrams/StepReveal'
import { Section } from '@lib/components/Section'
import { CLASSIFICATIONS, LIFECYCLE_STAGES } from '../content/lifecycle'

const TONE_TEXT = {
  accent: 'text-accent',
  alert: 'text-alert',
  warn: 'text-warn',
} as const

/**
 * A6 — the twelve-stage scenario lifecycle, then the six classifications with
 * their exit codes. The claim: no ambiguity about whose fault a red run is.
 */
export function LifecycleSection() {
  return (
    <Section
      id="lifecycle"
      index="05"
      eyebrow="integration · lifecycle"
      title="twelve stages, six classifications, no ambiguity about whose fault."
      lede="readiness is contract-based, never sleep-based, and the boot order is split so the controlled target is verified before the subject starts. when a run goes red, the classification says whether the subject broke a contract or the infrastructure failed; those exit differently."
    >
      <StepReveal title="scenario lifecycle" stages={LIFECYCLE_STAGES} />

      <div className="mt-6 border border-rule bg-bg">
        <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
          failure precedence: runner_error &gt; process_crash &gt; setup_error &gt; timeout &gt; contract_failure
        </div>
        <div className="grid grid-cols-1 @2xl:grid-cols-2 @4xl:grid-cols-3 gap-px bg-rule">
          {CLASSIFICATIONS.map((c) => (
            <div key={c.id} className="bg-bg px-4 py-3.5 min-w-0">
              <div className="flex items-baseline justify-between gap-x-3">
                <span className={`font-mono text-[13px] font-semibold ${TONE_TEXT[c.tone]}`}>{c.id}</span>
                <span className="font-mono text-[11px] text-ink-ghost tabular-nums">exit {c.exit}</span>
              </div>
              <p className="mt-1.5 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{c.desc}</p>
            </div>
          ))}
        </div>
      </div>
    </Section>
  )
}
