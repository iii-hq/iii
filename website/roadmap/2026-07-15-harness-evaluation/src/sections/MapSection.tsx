import { MapDatasheet, SystemMap } from '@lib/components/diagrams/SystemMap'
import { Section } from '@lib/components/Section'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { useEffect, useRef, useState } from 'react'
import { MAP_EDGES, MAP_INFO, MAP_NODES } from '../content/architecture'

/** matches tailwind @5xl container width (64rem) */
const PAIRED_LAYOUT_MIN_WIDTH = 1024

const LEGEND = [
  { swatch: <span className="inline-block size-3 border-[1.25px] border-ink bg-bg" />, label: 'track owner / boundary' },
  { swatch: <span className="inline-block size-3 border border-ink-faint bg-bg" />, label: 'controlled / evidence' },
  { swatch: <span className="inline-block size-3 border border-rule bg-bg" />, label: 'operator / production' },
  { swatch: <StatusDot pulse />, label: 'active flow' },
] as const

/**
 * A4 — the system map. One public boundary in the middle, the deterministic
 * track above it, the real-model track below it, evidence and graders shared.
 */
export function MapSection() {
  const [selected, setSelected] = useState('harness')
  const layoutRef = useRef<HTMLDivElement>(null)
  const mapRef = useRef<HTMLDivElement>(null)
  const [pairedLayout, setPairedLayout] = useState(false)
  const [mapHeight, setMapHeight] = useState<number | undefined>()

  useEffect(() => {
    const layoutEl = layoutRef.current
    const mapEl = mapRef.current
    if (!layoutEl || !mapEl) return

    const sync = () => {
      const paired = layoutEl.clientWidth >= PAIRED_LAYOUT_MIN_WIDTH
      setPairedLayout(paired)
      setMapHeight(paired ? mapEl.offsetHeight : undefined)
    }

    sync()
    const observer = new ResizeObserver(sync)
    observer.observe(layoutEl)
    observer.observe(mapEl)
    return () => observer.disconnect()
  }, [])

  const info = MAP_INFO[selected] ?? MAP_INFO.harness

  return (
    <Section
      id="map"
      index="03"
      eyebrow="system map"
      title="one public boundary, two tracks around it."
      lede="the integration runner and the e2e suite both drive the same harness through harness::send. only the far side of the router boundary differs. click any node to read its datasheet."
    >
      <div className="flex flex-wrap items-center gap-x-5 gap-y-2 mb-5">
        {LEGEND.map((item) => (
          <span key={item.label} className="flex items-center gap-x-2">
            {item.swatch}
            <span className="font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint">{item.label}</span>
          </span>
        ))}
      </div>

      <div ref={layoutRef} className="grid grid-cols-1 @5xl:grid-cols-[minmax(0,1fr)_340px] gap-6 items-stretch">
        <div ref={mapRef} className="border border-rule bg-bg p-3 overflow-x-auto min-h-0 self-start">
          <div className="min-w-[760px]">
            <SystemMap nodes={MAP_NODES} edges={MAP_EDGES} selected={selected} onSelect={setSelected} />
          </div>
        </div>
        <div
          className="@5xl:sticky @5xl:top-16 min-h-0 overflow-hidden"
          style={pairedLayout && mapHeight ? { height: mapHeight } : undefined}
        >
          <MapDatasheet
            info={info}
            className={pairedLayout ? 'h-full' : undefined}
            layoutKey={pairedLayout ? mapHeight : 'stack'}
          />
        </div>
      </div>
    </Section>
  )
}
