import { MapDatasheet, SystemMap } from '@lib/components/diagrams/SystemMap'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { useEffect, useRef, useState } from 'react'
import { MAP_EDGES, MAP_INFO, MAP_NODES } from '../content/architecture'

const PAIRED_LAYOUT_MIN_WIDTH = 1024

const LEGEND = [
  { swatch: <span className="inline-block size-3 border-[1.25px] border-ink bg-bg" />, label: 'core' },
  { swatch: <span className="inline-block size-3 border border-ink-faint bg-bg" />, label: 'track boundary' },
  { swatch: <span className="inline-block size-3 border border-rule bg-bg" />, label: 'external' },
  { swatch: <StatusDot pulse />, label: 'selected flow' },
] as const

export function MapSection() {
  const [selected, setSelected] = useState('harness')
  const layoutRef = useRef<HTMLDivElement>(null)
  const mapRef = useRef<HTMLDivElement>(null)
  const [pairedLayout, setPairedLayout] = useState(false)
  const [mapHeight, setMapHeight] = useState<number | undefined>()

  useEffect(() => {
    const layout = layoutRef.current
    const map = mapRef.current
    if (!layout || !map) return

    const sync = () => {
      const paired = layout.clientWidth >= PAIRED_LAYOUT_MIN_WIDTH
      setPairedLayout(paired)
      setMapHeight(paired ? map.offsetHeight : undefined)
    }

    sync()
    const observer = new ResizeObserver(sync)
    observer.observe(layout)
    observer.observe(map)
    return () => observer.disconnect()
  }, [])

  const info = MAP_INFO[selected] ?? MAP_INFO.harness

  return (
    <Section
      id="map"
      index="03"
      eyebrow="system map"
      title="the public path stays fixed. the model boundary changes by track."
      lede="click any node to inspect its responsibility. integration routes to a controlled script; quality routes to the production provider. both return to the same harness-owned durability and evidence boundaries."
    >
      <div className="mb-5 flex flex-wrap items-center gap-x-5 gap-y-2">
        {LEGEND.map((item) => (
          <span key={item.label} className="flex items-center gap-x-2">
            {item.swatch}
            <span className="font-mono text-[10px] tracking-[0.06em] text-ink-faint uppercase">{item.label}</span>
          </span>
        ))}
      </div>

      <div ref={layoutRef} className="grid grid-cols-1 items-stretch gap-6 @5xl:grid-cols-[minmax(0,1fr)_340px]">
        <div ref={mapRef} className="min-h-0 self-start overflow-x-auto border border-rule bg-bg p-3">
          <div className="min-w-[760px]">
            <SystemMap nodes={MAP_NODES} edges={MAP_EDGES} selected={selected} onSelect={setSelected} />
          </div>
        </div>
        <div
          className="min-h-0 overflow-hidden @5xl:sticky @5xl:top-16"
          style={pairedLayout && mapHeight ? { height: mapHeight } : undefined}
        >
          <MapDatasheet
            info={info}
            className={pairedLayout ? 'h-full' : undefined}
            layoutKey={pairedLayout ? mapHeight : 'stack'}
          />
        </div>
      </div>

      <div className="mt-6">
        <SpecSheet title="the boundary rule" meta="one run selects one branch">
          <SpecRow name="integration" type="scripted router">
            exact model requests and ordered frames make contract behavior reproducible without credentials.
          </SpecRow>
          <SpecRow name="quality / e2e" type="production router + provider">
            the pinned subject retains real inference, capability choice, latency, usage, and cost.
          </SpecRow>
          <SpecRow name="shared" type="public durable path">
            harness::send, queueing, status, transcript, domain effects, traces, classification, and cleanup remain
            authoritative.
          </SpecRow>
        </SpecSheet>
      </div>
    </Section>
  )
}
