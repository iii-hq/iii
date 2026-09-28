import { MapDatasheet, SystemMap } from "@lib/components/diagrams/SystemMap"
import { MapLayout, MapLegend } from "@lib/components/MapLayout"
import { Section } from "@lib/components/Section"
import { StatusDot } from "@lib/components/schematic/StatusDot"
import { useState } from "react"
import { MAP_EDGES, MAP_INFO, MAP_NODES } from "../content/architecture"

const LEGEND = [
  { swatch: <span className="inline-block size-3 border-[1.25px] border-ink bg-bg" />, label: "new in this spec" },
  { swatch: <span className="inline-block size-3 border border-ink-faint bg-bg" />, label: "engine, used as-is" },
  { swatch: <span className="inline-block size-3 border border-rule bg-bg" />, label: "author side" },
  { swatch: <StatusDot pulse />, label: "active flow" },
] as const

/**
 * A4 — the system map. Four processes, two new trigger types between them,
 * zero engine changes. Click a node for its datasheet.
 */
export function MapSection() {
  const [selected, setSelected] = useState("handler")

  const info = MAP_INFO[selected] ?? MAP_INFO.handler

  return (
    <Section
      id="map"
      index="03"
      eyebrow="system map"
      title="no new engine surface."
      lede="the console worker owns three new trigger types: two carry assets from workers, one carries each tab's subscription. everything in between is existing engine machinery (forwarding, parking, replay). click any node for its datasheet."
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
