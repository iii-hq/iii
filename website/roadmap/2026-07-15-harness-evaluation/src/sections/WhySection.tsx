import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { FnChip } from '@lib/components/schematic/FnChip'
import { EVALUATION_TRACKS, SHARED_CONTRACTS } from '../content/tracks'

export function WhySection() {
  return (
    <Section
      id="why"
      index="01"
      eyebrow="two tracks"
      title="one harness path answers two different questions."
      lede="integration isolates harness behavior from model variance. quality/E2E keeps real inference and asks whether one pinned subject achieves durable outcomes within explicit budgets. neither track can substitute for the other."
    >
      <div className="grid grid-cols-1 gap-px border border-rule bg-rule @4xl:grid-cols-2">
        {EVALUATION_TRACKS.map((track, index) => (
          <article key={track.id} className="min-w-0 bg-bg p-6 @3xl:p-8">
            <div className="flex items-center justify-between gap-3">
              <FnChip tone="accent">0{index + 1}</FnChip>
              <span className="font-mono text-[10px] tracking-[0.1em] text-ink-ghost uppercase">{track.label}</span>
            </div>
            <h3 className="mt-5 max-w-[30ch] font-mono text-[21px] font-semibold leading-[1.35] text-ink lowercase">
              {track.question}
            </h3>
            <dl className="mt-6 flex flex-col gap-4">
              {[
                ['model boundary', track.boundary],
                ['oracle', track.oracle],
                ['corpus', track.corpus],
                ['gate', track.cadence],
                ['output', track.output],
              ].map(([label, value]) => (
                <div key={label} className="border-t border-rule-2 pt-3">
                  <dt className="font-mono text-[10px] tracking-[0.08em] text-ink-ghost uppercase">{label}</dt>
                  <dd className="mt-1 font-mono text-[12.5px] leading-[1.6] text-ink-faint lowercase">{value}</dd>
                </div>
              ))}
            </dl>
          </article>
        ))}
      </div>

      <div className="mt-6">
        <SpecSheet title="what both tracks promise" meta="shared public contract">
          {SHARED_CONTRACTS.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
        </SpecSheet>
      </div>
    </Section>
  )
}
