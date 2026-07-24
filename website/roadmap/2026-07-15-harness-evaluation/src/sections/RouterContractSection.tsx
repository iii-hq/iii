import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { FnChip } from '@lib/components/schematic/FnChip'
import { QUALITY_EVIDENCE, QUALITY_OUTPUTS, QUALITY_SUBJECT_FIELDS } from '../content/quality'

export function RouterContractSection() {
  return (
    <Section
      id="contract"
      index="06"
      eyebrow="track 2 / quality e2e"
      title="pin the subject. keep the model real. grade the durable outcome."
      lede="quality/E2E runs one materially varied corpus against one resolved subject. the model may choose a valid trajectory; deterministic validators still decide correctness from domain state, harness evidence, trace completeness, and hard budgets."
    >
      <div className="grid grid-cols-1 gap-px border border-rule bg-rule @2xl:grid-cols-2 @5xl:grid-cols-5">
        {QUALITY_SUBJECT_FIELDS.map((row, index) => (
          <article key={row.name} className="min-w-0 bg-bg p-5">
            <div className="flex items-center justify-between gap-3">
              <FnChip>{row.name}</FnChip>
              <span className="font-mono text-[10px] text-ink-ghost tabular-nums">0{index + 1}</span>
            </div>
            <p className="mt-3 font-mono text-[10px] tracking-[0.08em] text-ink-ghost uppercase">{row.type}</p>
            <p className="mt-2 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{row.desc}</p>
          </article>
        ))}
      </div>

      <div className="mt-6 grid grid-cols-1 gap-4 @4xl:grid-cols-2">
        <SpecSheet title="evidence composition" meta="complete root + descendants">
          {QUALITY_EVIDENCE.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
        </SpecSheet>

        <SpecSheet title="two outputs, not one score" meta="same execution">
          {QUALITY_OUTPUTS.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
          <div className="mt-4">
            <a
              href="#/quality"
              className="font-mono text-[12px] text-ink-faint lowercase transition-colors hover:text-accent"
            >
              inspect the quality protocol →
            </a>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
