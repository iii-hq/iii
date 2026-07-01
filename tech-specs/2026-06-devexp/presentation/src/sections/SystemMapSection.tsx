import { useState } from 'react'
import { MapDatasheet, SystemMap } from '@/components/diagrams/SystemMap'
import { Section } from '@/components/Section'
import { KIND_HUE, MEETING_POINTS } from '@/content/map'

const LEGEND: Array<{ kind: keyof typeof KIND_HUE; label: string }> = [
  { kind: 'plane', label: 'connection plane' },
  { kind: 'brain', label: 'the brain' },
  { kind: 'pid', label: 'pid owner' },
  { kind: 'file', label: 'file' },
  { kind: 'wk', label: 'worker' },
]

export function SystemMapSection() {
  const [selected, setSelected] = useState('ops')

  return (
    <Section
      id="map"
      index="03"
      eyebrow="how it works"
      title="four planes, three meeting points."
      lede="the engine never spawns; the daemon never decides what to run; the brain never holds a pid. click any node to read its datasheet — the three numbered arrows are the only places the planes touch."
    >
      <div className="flex flex-wrap items-center gap-x-5 gap-y-2 mb-5">
        {LEGEND.map((item) => (
          <span key={item.kind} className="flex items-center gap-x-2">
            <span
              className="inline-block size-2.5 rounded-full"
              style={{ background: KIND_HUE[item.kind] }}
            />
            <span className="font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint">
              {item.label}
            </span>
          </span>
        ))}
        <span className="flex items-center gap-x-2">
          <span className="inline-block w-5 border-t border-dashed border-ink-faint" />
          <span className="font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint">
            bootstrap floor
          </span>
        </span>
      </div>

      <div className="grid grid-cols-1 @5xl:grid-cols-[minmax(0,1fr)_340px] gap-6 items-start">
        <div className="border border-rule bg-bg p-3 overflow-x-auto">
          <div className="min-w-[760px]">
            <SystemMap selected={selected} onSelect={setSelected} />
          </div>
        </div>
        <div className="@5xl:sticky @5xl:top-16">
          <MapDatasheet selected={selected} />
        </div>
      </div>

      <div className="mt-6 grid grid-cols-1 @3xl:grid-cols-3 gap-px bg-rule border border-rule">
        {MEETING_POINTS.map((mp) => (
          <div key={mp.badge} className="bg-bg p-5">
            <div className="flex items-center gap-x-2.5 mb-2">
              <span className="inline-flex items-center justify-center size-6 border border-accent text-accent font-mono text-[13px]">
                {mp.badge}
              </span>
              <span className="font-mono text-[12px] text-ink-faint">{mp.from}</span>
            </div>
            <div className="font-mono text-[14px] font-semibold lowercase text-ink mb-2">
              {mp.title}
            </div>
            <p className="font-mono text-[12px] leading-[1.65] text-ink-faint lowercase">
              {mp.detail}
            </p>
          </div>
        ))}
      </div>
    </Section>
  )
}
