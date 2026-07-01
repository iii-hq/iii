import { MapDatasheet, SystemMap } from '@lib/components/diagrams/SystemMap'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { useEffect, useRef, useState } from 'react'
import { MAP_EDGES, MAP_INFO, MAP_NODES } from '../content/example'

/** matches tailwind @5xl container width (64rem) */
const PAIRED_LAYOUT_MIN_WIDTH = 1024

const LEGEND = [
  { swatch: <span className="inline-block size-3 border-[1.25px] border-ink bg-bg" />, label: 'core' },
  { swatch: <span className="inline-block size-3 border border-ink-faint bg-bg" />, label: 'supporting' },
  { swatch: <span className="inline-block size-3 border border-dashed border-ink-faint bg-bg" />, label: 'optional' },
  { swatch: <span className="inline-block size-3 border border-rule bg-bg" />, label: 'external' },
  { swatch: <StatusDot pulse />, label: 'active flow' },
] as const

export function SystemMapSection() {
  const [selected, setSelected] = useState('gateway')
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

  const info = MAP_INFO[selected] ?? MAP_INFO.gateway

  return (
    <Section
      id="map"
      index="01"
      eyebrow="system map"
      title="a few parts, talking only over typed contracts."
      lede="every arrow below is a call or an event — no hidden in-process coupling anywhere. click any node to read its datasheet: each part is useful on its own, and together they are the whole system."
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

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="the wiring, in words" meta={`${MAP_EDGES.length} edges`}>
          <div className="flex flex-col">
            <SpecRow name="client → gateway::request" type="work in">
              any caller drops a request and gets an id back immediately.
            </SpecRow>
            <SpecRow name="gateway → worker::run" type="dispatch">
              queued work is handed to a worker to execute, in order.
            </SpecRow>
            <SpecRow name="gateway / worker → store" type="persist + emit">
              every write lands in the durable store and fires an event.
            </SpecRow>
            <SpecRow name="store → watcher" type="reactive surface">
              optional reactors bind store events and render or act live.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="design principles" meta="from the spec">
          <div className="flex flex-col">
            <SpecRow name="standalone first">
              every part is independently installable and useful by itself. cross-part calls are explicit, never
              imports.
            </SpecRow>
            <SpecRow name="one job each">
              the gateway routes, the worker runs, the store remembers. anything that grows real logic becomes its own
              part.
            </SpecRow>
            <SpecRow name="reactive by default">
              state changes emit events; consumers bind once and render live.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
