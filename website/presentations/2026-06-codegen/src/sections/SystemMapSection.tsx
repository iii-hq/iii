import { MapDatasheet, SystemMap } from '@lib/components/diagrams/SystemMap'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { useEffect, useRef, useState } from 'react'
import { MAP_EDGES, MAP_INFO, MAP_NODES } from '../content/map'

/** matches tailwind @5xl container width (64rem) */
const PAIRED_LAYOUT_MIN_WIDTH = 1024

const LEGEND = [
  { swatch: <span className="inline-block size-3 border-[1.25px] border-ink bg-bg" />, label: 'the tool' },
  { swatch: <span className="inline-block size-3 border border-ink-faint bg-bg" />, label: 'engine + output' },
  { swatch: <span className="inline-block size-3 border border-rule bg-bg" />, label: 'your side' },
  { swatch: <StatusDot pulse />, label: 'active flow' },
] as const

export function SystemMapSection() {
  const [selected, setSelected] = useState('codegen')
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

  const info = MAP_INFO[selected] ?? MAP_INFO.codegen

  return (
    <Section
      id="map"
      index="03"
      eyebrow="system map"
      title="config in, live catalog read, typed files out."
      lede="the whole tool is one binary between three things it doesn't own: your config, the engine's catalog, and your repo. click any node to read its datasheet."
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
        <SpecSheet title="the pipeline" meta="5 stages">
          <div className="flex flex-col">
            <SpecRow name="select" type="globs → ids">
              resolve your workers / functions / triggers globs against the live catalog into a concrete set of ids.
            </SpecRow>
            <SpecRow name="discover" type="engine::*::info">
              pull the request and response json schema for every selected id.
            </SpecRow>
            <SpecRow name="map" type="schema → types">
              project json schema into idiomatic types, collecting nested $defs.
            </SpecRow>
            <SpecRow name="emit" type="types + wrappers">
              render types, function wrappers, and trigger helpers per mode.
            </SpecRow>
            <SpecRow name="write" type="deterministic">
              sorted, banner-stamped output; identical input is a no-op diff.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="what codegen does not do" meta="boundaries">
          <div className="flex flex-col">
            <SpecRow name="not a scaffolder">
              it generates the client surface, never the worker's function bodies.
            </SpecRow>
            <SpecRow name="not a schema author">
              schemas belong to the workers that register them; codegen only reads.
            </SpecRow>
            <SpecRow name="live catalog only" type="v1">
              targets must be connected to be discovered; a snapshot mode is v2.
            </SpecRow>
            <SpecRow name="go" type="reserved">
              typescript, javascript, rust, python today; go is planned.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
