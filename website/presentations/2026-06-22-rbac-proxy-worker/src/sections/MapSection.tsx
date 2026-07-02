import { MapDatasheet, SystemMap } from '@lib/components/diagrams/SystemMap'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { useEffect, useRef, useState } from 'react'
import { MAP_EDGES, MAP_INFO, MAP_NODES } from '../content/architecture'

/** matches tailwind @5xl container width (64rem) */
const PAIRED_LAYOUT_MIN_WIDTH = 1024

const LEGEND = [
  { swatch: <span className="inline-block size-3 border-[1.25px] border-ink bg-bg" />, label: 'proxy core' },
  { swatch: <span className="inline-block size-3 border border-ink-faint bg-bg" />, label: 'proxy relay' },
  { swatch: <span className="inline-block size-3 border border-rule bg-bg" />, label: 'trusted / untrusted' },
  { swatch: <StatusDot pulse />, label: 'active flow' },
] as const

export function MapSection() {
  const [selected, setSelected] = useState('interceptor')
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

  const info = MAP_INFO[selected] ?? MAP_INFO.interceptor

  return (
    <Section
      id="map"
      index="03"
      eyebrow="system map"
      title="one boundary, two connection planes."
      lede="untrusted workers reach only the public port; the engine, its config, and the policy functions stay on the trusted network. click any node to read its datasheet."
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
        <SpecSheet title="the two connection planes" meta="control + data">
          <div className="flex flex-col">
            <SpecRow name="control connection" type="one, persistent">
              the proxy&apos;s own worker identity. registers rbac-proxy::status, invokes auth / middleware / hooks,
              runs the catalog caches, and integrates with configuration.
            </SpecRow>
            <SpecRow name="data connection" type="one per downstream">
              a fresh upstream websocket per inbound worker, frames pumped both ways through the interceptor.
            </SpecRow>
            <SpecRow name="cleanup" type="inherited">
              downstream close → upstream close; the engine&apos;s per-connection teardown removes that
              connection&apos;s functions and triggers.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="what it never does" meta="pure boundary">
          <div className="flex flex-col">
            <SpecRow name="touch the engine" type="no">
              no engine, protocol, or port changes; worker code against the existing Message protocol.
            </SpecRow>
            <SpecRow name="persist state" type="no">
              only transient per-connection sessions and short-ttl caches.
            </SpecRow>
            <SpecRow name="replace engine rbac" type="no">
              an alternative home for the same rules; run the gateway, the proxy, or both.
            </SpecRow>
            <SpecRow name="re-issue access_keys" type="no">
              channel sockets are relayed; the engine validates the capability token.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
