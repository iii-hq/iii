import { PageShell } from '@lib/components/PageShell'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { FnChip } from '@lib/components/schematic/FnChip'
import { QUALITY_EVIDENCE, QUALITY_OUTPUTS, QUALITY_SCENARIOS, QUALITY_SUBJECT_FIELDS } from '../content/quality'

export function QualityPage() {
  return (
    <PageShell
      eyebrow="deep dive / quality e2e"
      title="one pinned subject. five real-model tests. two outputs."
      description="the quality track keeps production inference, grades durable outcomes with deterministic code, attributes the complete root-and-descendant run, and reports efficiency without reducing it to one score."
      related={[
        { slug: 'protocol', label: 'integration protocol' },
        { slug: 'scenarios', label: 'both scenario corpora' },
      ]}
    >
      <SpecSheet title="resolved subject" meta="one identity per run">
        {QUALITY_SUBJECT_FIELDS.map((row) => (
          <SpecRow key={row.name} name={row.name} type={row.type}>
            {row.desc}
          </SpecRow>
        ))}
      </SpecSheet>

      <div className="grid grid-cols-1 gap-4 @4xl:grid-cols-2">
        <SpecSheet title="public evidence" meta="fail closed">
          {QUALITY_EVIDENCE.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
        </SpecSheet>

        <SpecSheet title="run result" meta="correctness stays separate">
          {QUALITY_OUTPUTS.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
        </SpecSheet>
      </div>

      <div className="grid grid-cols-1 gap-px border border-rule bg-rule @4xl:grid-cols-2 @5xl:grid-cols-3">
        {QUALITY_SCENARIOS.map((scenario) => (
          <article key={scenario.id} className="bg-bg p-5">
            <div className="flex items-center justify-between gap-3">
              <FnChip tone="accent">family {scenario.id}</FnChip>
              <span className="font-mono text-[10px] tracking-[0.08em] text-ink-ghost uppercase">real model</span>
            </div>
            <h2 className="mt-4 font-mono text-[16px] font-semibold text-ink lowercase">{scenario.slug}</h2>
            <p className="mt-3 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{scenario.outcome}</p>
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
    </PageShell>
  )
}
