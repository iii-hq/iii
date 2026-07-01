import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { useEffect, useRef, useState } from 'react'
import { MapDatasheet, SystemMap } from '../diagrams/SystemMap'

/** matches tailwind @5xl container width (64rem) */
const PAIRED_LAYOUT_MIN_WIDTH = 1024

const LEGEND = [
  { swatch: <span className="inline-block size-3 border-[1.25px] border-ink bg-bg" />, label: 'core worker' },
  {
    swatch: <span className="inline-block size-3 border border-dashed border-ink-faint bg-bg" />,
    label: 'optional sibling',
  },
  { swatch: <span className="inline-block size-3 border border-rule bg-bg" />, label: 'example consumer' },
  { swatch: <span className="inline-block size-3 border border-rule bg-paper-2" />, label: 'the substrate' },
  { swatch: <StatusDot pulse />, label: 'active flow' },
] as const

export function SystemMapSection() {
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

  return (
    <Section
      id="map"
      index="02"
      eyebrow="system map"
      title="four core workers, one optional sibling — talking only over the bus."
      lede="no in-process coupling anywhere: every arrow below is a function call or an event on the iii bus. click any node to read its datasheet — each worker installs alone and earns its place alone; together they are the full agent loop."
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
            <SystemMap selected={selected} onSelect={setSelected} />
          </div>
        </div>
        <div
          className="@5xl:sticky @5xl:top-16 min-h-0 overflow-hidden"
          style={pairedLayout && mapHeight ? { height: mapHeight } : undefined}
        >
          <MapDatasheet
            selected={selected}
            className={pairedLayout ? 'h-full' : undefined}
            layoutKey={pairedLayout ? mapHeight : 'stack'}
          />
        </div>
      </div>

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="the wiring, in words" meta="8 edges">
          <div className="flex flex-col">
            <SpecRow name="consumer → harness::send" type="message in">
              a chat, a webhook bridge, or any worker drops a message and gets
              {' { session_id, turn_id } '} back immediately.
            </SpecRow>
            <SpecRow name="harness → session::append / update_message" type="persist + stream">
              the loop writes every delta into the session store; each write emits an event consumers already listen to.
            </SpecRow>
            <SpecRow name="harness → context::assemble" type="fit the window">
              raw history in, model-ready context out — pruned, compacted, budgeted for the exact target model.
            </SpecRow>
            <SpecRow name="harness → router::chat" type="generate">
              one front door for every provider; the stream relays back over a channel frame by frame.
            </SpecRow>
            <SpecRow name="harness → agent_trigger" type="dispatch">
              the model asks for work; the harness runs it against any allowed function on the bus — fail-closed.
            </SpecRow>
            <SpecRow name="approval-gate ⇆ harness" type="hold / release">
              a held call parks the turn; a human decision releases or answers it. the loop never changes.
            </SpecRow>
            <SpecRow name="harness → harness::spawn" type="sub-agents">
              children are ordinary sessions on the same queue — parallel by construction.
            </SpecRow>
            <SpecRow name="session-manager → consumers" type="reactive surface">
              session created, message added / updated, status changed — bind once, render live everywhere.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="design principles" meta="from the spec">
          <div className="flex flex-col">
            <SpecRow name="standalone first">
              every core worker is independently installable and has a coherent purpose by itself. cross-worker calls
              are explicit bus calls, never imports.
            </SpecRow>
            <SpecRow name="the harness is thin">
              it sequences the other three plus function dispatch. anything that grows real logic — approvals, budgets,
              scheduling — becomes a sibling, never harness code.
            </SpecRow>
            <SpecRow name="llm-router is consumer-agnostic">
              it never assumes a loop, a session, or a ui. it streams into a caller-supplied channel and returns.
            </SpecRow>
            <SpecRow name="context-manager owns no storage">
              message arrays in, results out; the caller persists. reusable by any harness or ai feature.
            </SpecRow>
            <SpecRow name="session-manager is the single reactive surface">
              consumers bind its trigger types and render live — they never chase status and never need to know about
              the provider or the loop.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
