import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { ModeToggle } from '@lib/components/schematic/ModeToggle'
import { useState } from 'react'
import { ADJACENT_SYSTEMS, TRACKS, type TrackId } from '../content/tracks'

/**
 * A9 — the two-track split as a toggle. Flip between conformance and agent
 * quality and watch the whole profile swap: boundary, oracle, owner, first
 * use, release policy. The claim is the separation itself.
 */
export function TracksSection() {
  const [trackId, setTrackId] = useState<TrackId>('conformance')
  const track = TRACKS.find((t) => t.id === trackId) ?? TRACKS[0]

  return (
    <Section
      id="tracks"
      index="02"
      eyebrow="the split"
      title="same vocabulary. never the same oracle."
      lede="both tracks enter through harness::send as ordinary public turns, name evidence the same way, and report in the same conventions. everything else (oracle, execution owner, release policy) is deliberately unshared."
    >
      <div className="border border-rule bg-bg">
        <div className="flex flex-wrap items-center justify-between gap-3 bg-panel px-3.5 py-2.5 border-b border-rule">
          <ModeToggle
            value={trackId}
            onChange={(v) => setTrackId(v)}
            options={TRACKS.map((t) => ({ value: t.id, label: t.label }))}
          />
          <span className="font-mono text-[12px] lowercase text-ink-faint">{track.headline}</span>
        </div>

        <div className="flex flex-col">
          {track.rows.map((row) => (
            <div
              key={row.k}
              className="grid grid-cols-1 @3xl:grid-cols-[200px_260px_minmax(0,1fr)] gap-x-6 gap-y-1 px-4 py-3.5 border-b border-rule-2 last:border-b-0"
            >
              <span className="font-mono text-[11px] uppercase tracking-[0.08em] text-ink-faint self-baseline pt-0.5">
                {row.k}
              </span>
              <span className="font-mono text-[13px] font-semibold text-ink lowercase">{row.v}</span>
              <span className="font-mono text-[12.5px] leading-[1.6] text-ink-faint lowercase">{row.detail}</span>
            </div>
          ))}
        </div>
      </div>

      <div className="mt-8">
        <SpecSheet title="what this is deliberately not" meta="three adjacent systems">
          <div className="flex flex-col">
            {ADJACENT_SYSTEMS.map((row) => (
              <SpecRow key={row.name} name={row.name} type={row.type}>
                {row.desc}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
