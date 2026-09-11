import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { FnChip } from '@lib/components/schematic/FnChip'
import { ModeToggle } from '@lib/components/schematic/ModeToggle'
import { useState } from 'react'
import { SCENARIO_TRACKS, type ScenarioTrack } from '../content/scenarios'

export function ScenariosSection() {
  const [track, setTrack] = useState<ScenarioTrack>('integration')
  const scenarios = SCENARIO_TRACKS[track]
  const integration = track === 'integration'

  return (
    <Section
      id="scenarios"
      index="08"
      eyebrow="scenario corpora"
      title="four fixtures prove contracts. five families evaluate capability."
      lede="the integration corpus is intentionally small and exact. the quality corpus is materially varied so a pinned subject must respond, call, delegate, react, and reduce while preserving durable outcomes and complete attribution."
    >
      <div className="mb-5 flex flex-wrap items-center justify-between gap-3">
        <ModeToggle
          value={track}
          onChange={setTrack}
          options={[
            { value: 'integration', label: 'integration · 4' },
            { value: 'quality', label: 'quality / e2e · 5' },
          ]}
        />
        <a
          href="#/scenarios"
          className="font-mono text-[12px] text-ink-faint lowercase transition-colors hover:text-accent"
        >
          open both corpora →
        </a>
      </div>

      <div className="grid grid-cols-1 gap-px border border-rule bg-rule @4xl:grid-cols-2 @6xl:grid-cols-3">
        {scenarios.map((scenario) => (
          <article key={scenario.id} className="min-w-0 bg-bg p-5">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <FnChip tone="accent">{integration ? scenario.id : `family ${scenario.id}`}</FnChip>
              <span className="font-mono text-[10px] tracking-[0.08em] text-ink-ghost uppercase">{scenario.meta}</span>
            </div>
            <h3 className="mt-4 font-mono text-[16px] font-semibold text-ink lowercase">{scenario.slug}</h3>
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

      <div className="mt-6">
        <SpecSheet title="corpus policy" meta="different by design">
          <SpecRow name="integration" type="exact and checked in">
            four fixtures own exact router scripts, terminal-turn counts, and trace assertions.
          </SpecRow>
          <SpecRow name="quality / e2e" type="same corpus, different subject">
            scenario source stays unchanged when the model, prompt, skills, harness, or worker artifacts change.
          </SpecRow>
          <SpecRow name="trajectory" type="only when contractually required">
            quality checks durable outcomes and budgets without demanding one exact Function sequence when several are
            valid.
          </SpecRow>
        </SpecSheet>
      </div>
    </Section>
  )
}
