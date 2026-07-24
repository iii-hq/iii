import { Funnel } from '@lib/components/diagrams/Funnel'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { ModeToggle } from '@lib/components/schematic/ModeToggle'
import { useState } from 'react'
import {
  COMMON_EVIDENCE_RULES,
  type EvidenceTrack,
  INTEGRATION_EVIDENCE_PATHS,
  INTEGRATION_FLOOR_ROWS,
  QUALITY_EVIDENCE_PATHS,
  QUALITY_FLOOR_ROWS,
} from '../content/evidence'

export function EvidenceSection() {
  const [track, setTrack] = useState<EvidenceTrack>('integration')
  const integration = track === 'integration'
  const paths = integration ? INTEGRATION_EVIDENCE_PATHS : QUALITY_EVIDENCE_PATHS
  const floor = integration ? INTEGRATION_FLOOR_ROWS : QUALITY_FLOOR_ROWS

  return (
    <Section
      id="evidence"
      index="07"
      eyebrow="oracles"
      title="shared evidence does not mean a shared oracle."
      lede="both tracks trust durable public evidence and fail closed on incomplete traces. integration demands exact contract history; quality composes domain outcomes, complete session attribution, and budget dimensions around a real-model run."
    >
      <div className="mb-5 flex flex-wrap items-center justify-between gap-3">
        <ModeToggle
          value={track}
          onChange={setTrack}
          options={[
            { value: 'integration', label: 'integration oracle' },
            { value: 'quality', label: 'quality oracle' },
          ]}
        />
        <span className="font-mono text-[11px] tracking-[0.08em] text-ink-ghost uppercase">
          {integration ? 'exact history' : 'outcome + attribution + budgets'}
        </span>
      </div>

      <Funnel
        title={`${integration ? 'integration' : 'quality'} evidence → track verdict`}
        paths={paths}
        target={{
          label: integration ? 'contract grading floor' : 'correctness + benchmark',
          sub: 'then scenario-specific checks',
        }}
        reject={{ label: 'self-report + private logs', desc: 'never sufficient' }}
      />

      <div className="mt-6 grid grid-cols-1 gap-4 @4xl:grid-cols-2">
        <SpecSheet title={`${integration ? 'integration' : 'quality'} floor`} meta="track-specific">
          {floor.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
        </SpecSheet>

        <SpecSheet title="shared evidence rules" meta="both tracks">
          {COMMON_EVIDENCE_RULES.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
        </SpecSheet>
      </div>
    </Section>
  )
}
