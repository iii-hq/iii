import { PageShell } from '@lib/components/PageShell'
import { FnChip } from '@lib/components/schematic/FnChip'
import { type EvaluationScenario, INTEGRATION_SCENARIOS, QUALITY_EVALUATION_SCENARIOS } from '../content/scenarios'

function ScenarioGrid({ scenarios, quality = false }: { scenarios: EvaluationScenario[]; quality?: boolean }) {
  return (
    <div className="grid grid-cols-1 gap-px border border-rule bg-rule @4xl:grid-cols-2 @5xl:grid-cols-3">
      {scenarios.map((scenario) => (
        <article key={scenario.id} className="bg-bg p-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <FnChip tone="accent">{quality ? `family ${scenario.id}` : scenario.id}</FnChip>
            <span className="font-mono text-[10px] tracking-[0.08em] text-ink-ghost uppercase">{scenario.meta}</span>
          </div>
          <h2 className="mt-4 font-mono text-[16px] font-semibold text-ink lowercase">{scenario.slug}</h2>
          <p className="mt-3 font-mono text-[11.5px] leading-[1.55] text-ink-ghost">{scenario.stimulus}</p>
          <p className="mt-4 border-l border-accent pl-3 font-mono text-[12.5px] leading-[1.6] text-ink">
            {scenario.outcome}
          </p>
          <ul className="mt-4 flex flex-col gap-2">
            {scenario.proof.map((proof) => (
              <li key={proof} className="font-mono text-[11px] leading-[1.5] text-ink-faint">
                <span className="mr-2 text-accent">✓</span>
                {proof}
              </li>
            ))}
          </ul>
        </article>
      ))}
    </div>
  )
}

export function ScenariosPage() {
  return (
    <PageShell
      eyebrow="deep dive / corpora"
      title="nine scenarios, grouped by the question they answer"
      description="integration fixtures are exact protocol proofs. quality families preserve real-model choice while fixing the subject identity, durable outcome, complete evidence contract, and resource ceilings."
      related={[
        { slug: 'protocol', label: 'integration protocol' },
        { slug: 'quality', label: 'quality / e2e protocol' },
      ]}
    >
      <div>
        <p className="mb-4 font-mono text-[11px] tracking-[0.12em] text-ink-ghost uppercase">
          integration · 4 fixtures
        </p>
        <ScenarioGrid scenarios={INTEGRATION_SCENARIOS} />
      </div>

      <div>
        <p className="mb-4 font-mono text-[11px] tracking-[0.12em] text-ink-ghost uppercase">
          quality / e2e · 5 families
        </p>
        <ScenarioGrid scenarios={QUALITY_EVALUATION_SCENARIOS} quality />
      </div>
    </PageShell>
  )
}
