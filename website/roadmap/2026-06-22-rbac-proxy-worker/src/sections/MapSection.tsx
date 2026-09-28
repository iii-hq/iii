import { MapDatasheet, SystemMap } from "@lib/components/diagrams/SystemMap"
import { MapLayout, MapLegend } from "@lib/components/MapLayout"
import { Section } from "@lib/components/Section"
import { SpecRow, SpecSheet } from "@lib/components/SpecSheet"
import { StatusDot } from "@lib/components/schematic/StatusDot"
import { useState } from "react"
import { MAP_EDGES, MAP_INFO, MAP_NODES } from "../content/architecture"

const LEGEND = [
  { swatch: <span className="inline-block size-3 border-[1.25px] border-ink bg-bg" />, label: "proxy core" },
  { swatch: <span className="inline-block size-3 border border-ink-faint bg-bg" />, label: "proxy relay" },
  { swatch: <span className="inline-block size-3 border border-rule bg-bg" />, label: "trusted / untrusted" },
  { swatch: <StatusDot pulse />, label: "active flow" },
] as const

export function MapSection() {
  const [selected, setSelected] = useState("interceptor")

  const info = MAP_INFO[selected] ?? MAP_INFO.interceptor

  return (
    <Section
      id="map"
      index="03"
      eyebrow="system map"
      title="one boundary, two connection planes."
      lede="untrusted workers reach only the public port; the engine, its config, and the policy functions stay on the trusted network. click any node to read its datasheet."
    >
      <MapLegend items={LEGEND} />

      <MapLayout
        map={<SystemMap nodes={MAP_NODES} edges={MAP_EDGES} selected={selected} onSelect={setSelected} />}
        datasheet={({ className, layoutKey }) => (
          <MapDatasheet info={info} className={className} layoutKey={layoutKey} />
        )}
      />

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
