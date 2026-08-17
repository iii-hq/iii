import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { GATE_ROWS } from '../content/transition'

export function TransitionSection() {
  return (
    <Section
      id="transition"
      index="10"
      eyebrow="gate policy"
      title="run both tracks, but do not force them into the same cadence."
      lede="integration is the fast contract gate. quality/E2E is the real-model outcome and budget gate. release confidence comes from their intersection, not from renaming either one as the complete answer."
    >
      <div className="overflow-x-auto border border-rule">
        <div className="min-w-[860px]">
          <div className="grid grid-cols-[170px_1fr_1fr_1.25fr] border-b border-rule bg-panel px-4 py-2.5">
            <span className="font-mono text-[10px] tracking-[0.1em] text-ink-ghost uppercase">moment</span>
            <span className="font-mono text-[10px] tracking-[0.1em] text-ink-ghost uppercase">integration</span>
            <span className="font-mono text-[10px] tracking-[0.1em] text-ink-ghost uppercase">quality / e2e</span>
            <span className="font-mono text-[10px] tracking-[0.1em] text-ink-ghost uppercase">why</span>
          </div>
          {GATE_ROWS.map((row) => (
            <div
              key={row.moment}
              className="grid grid-cols-[170px_1fr_1fr_1.25fr] border-b border-rule-2 last:border-b-0"
            >
              <p className="px-4 py-4 font-mono text-[12px] font-semibold text-ink lowercase">{row.moment}</p>
              <p className="border-l border-rule-2 px-4 py-4 font-mono text-[11.5px] leading-[1.55] text-ink-faint lowercase">
                {row.integration}
              </p>
              <p className="border-l border-rule-2 px-4 py-4 font-mono text-[11.5px] leading-[1.55] text-ink-faint lowercase">
                {row.quality}
              </p>
              <p className="border-l border-rule-2 px-4 py-4 font-mono text-[11.5px] leading-[1.55] text-ink lowercase">
                {row.reason}
              </p>
            </div>
          ))}
        </div>
      </div>

      <div className="mt-6">
        <SpecSheet title="v1 boundaries" meta="honest limits">
          <SpecRow name="integration" type="implemented contract">
            deterministic runner, direct and playground fixtures, exact router scripts, trace floor, reports, and
            teardown.
          </SpecRow>
          <SpecRow name="quality / e2e" type="evaluation contract">
            pinned subject, five real-model families, deterministic validators, complete attribution, and raw benchmark
            dimensions.
          </SpecRow>
          <SpecRow name="outside v1" type="comparison service">
            paired scheduling, statistics, aggregate scoring, comparison UI, held-out validators, and production-session
            evaluation.
          </SpecRow>
        </SpecSheet>
      </div>
    </Section>
  )
}
