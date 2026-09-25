import { MapDatasheet, SystemMap } from "@lib/components/diagrams/SystemMap"
import { MapLayout, MapLegend } from "@lib/components/MapLayout"
import { Section } from "@lib/components/Section"
import { SpecRow, SpecSheet } from "@lib/components/SpecSheet"
import { StatusDot } from "@lib/components/schematic/StatusDot"
import { useState } from "react"
import { MAP_EDGES, MAP_INFO, MAP_NODES } from "../content/map"

const LEGEND = [
  { swatch: <span className="inline-block size-3 border-[1.25px] border-ink bg-bg" />, label: "the tool" },
  { swatch: <span className="inline-block size-3 border border-ink-faint bg-bg" />, label: "engine + output" },
  { swatch: <span className="inline-block size-3 border border-rule bg-bg" />, label: "your side" },
  { swatch: <StatusDot pulse />, label: "active flow" },
] as const

export function SystemMapSection() {
  const [selected, setSelected] = useState("codegen")

  const info = MAP_INFO[selected] ?? MAP_INFO.codegen

  return (
    <Section
      id="map"
      index="03"
      eyebrow="system map"
      title="config in, live catalog read, typed files out."
      lede="the whole tool is one binary between three things it doesn't own: your config, the engine's catalog, and your repo. click any node to read its datasheet."
    >
      <MapLegend items={LEGEND} />

      <MapLayout
        map={<SystemMap nodes={MAP_NODES} edges={MAP_EDGES} selected={selected} onSelect={setSelected} />}
        datasheet={({ className, layoutKey }) => (
          <MapDatasheet info={info} className={className} layoutKey={layoutKey} />
        )}
      />

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
