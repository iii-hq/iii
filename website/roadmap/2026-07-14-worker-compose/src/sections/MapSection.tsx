import { MapDatasheet, SystemMap } from "@lib/components/diagrams/SystemMap"
import { MapLayout, MapLegend } from "@lib/components/MapLayout"
import { Section } from "@lib/components/Section"
import { StatusDot } from "@lib/components/schematic/StatusDot"
import { useState } from "react"
import { MAP_EDGES, MAP_INFO, MAP_NODES } from "../content/map"

const LEGEND = [
  { swatch: <span className="inline-block size-3 border-[1.25px] border-ink bg-bg" />, label: "engine / daemon" },
  { swatch: <span className="inline-block size-3 border border-ink-faint bg-bg" />, label: "worker / builtin" },
  { swatch: <StatusDot pulse />, label: "active flow" },
] as const

/**
 * A4 — the architecture in one navigable map: the engine routes and
 * arbitrates, each daemon supervises only its own children.
 */
export function MapSection() {
  const [selected, setSelected] = useState("daemon-a")

  const info = MAP_INFO[selected] ?? MAP_INFO["daemon-a"]

  return (
    <Section
      id="map"
      index="03"
      eyebrow="system map"
      title="the engine routes. the daemon supervises."
      lede="two machines, one engine. each daemon owns exactly the processes it spawned; the engine arbitrates names, buffers registrations, and answers who runs what. click any node."
    >
      <MapLegend items={LEGEND} />

      <MapLayout
        map={<SystemMap nodes={MAP_NODES} edges={MAP_EDGES} selected={selected} onSelect={setSelected} />}
        datasheet={({ className, layoutKey }) => (
          <MapDatasheet info={info} className={className} layoutKey={layoutKey} />
        )}
      />
    </Section>
  )
}
