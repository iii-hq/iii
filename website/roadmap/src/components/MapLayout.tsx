import { type ReactNode, useEffect, useRef, useState } from "react"

/** matches tailwind @5xl container width (64rem) */
const PAIRED_LAYOUT_MIN_WIDTH = 1024

export interface MapLegendItem {
  swatch: ReactNode
  label: string
}

/** the swatch + label strip above a system map */
export function MapLegend({ items }: { items: readonly MapLegendItem[] }) {
  return (
    <div className="flex flex-wrap items-center gap-x-5 gap-y-2 mb-5">
      {items.map((item) => (
        <span key={item.label} className="flex items-center gap-x-2">
          {item.swatch}
          <span className="font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint">{item.label}</span>
        </span>
      ))}
    </div>
  )
}

/** what the datasheet slot learns about the layout it sits in */
export interface MapDatasheetSlot {
  /** `h-full` once the map and datasheet sit side by side; undefined stacked */
  className: string | undefined
  /** remount key for the datasheet's scroll panel: the paired height, or "stack" */
  layoutKey: string | number
}

/**
 * the system-map slide's two-column body: the scrollable map on the left and
 * a sticky datasheet on the right, height-locked to the map once the
 * container is wide enough (@5xl); stacked below it otherwise.
 */
export function MapLayout({ map, datasheet }: { map: ReactNode; datasheet: (slot: MapDatasheetSlot) => ReactNode }) {
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
    <div ref={layoutRef} className="grid grid-cols-1 @5xl:grid-cols-[minmax(0,1fr)_340px] gap-6 items-stretch">
      <div ref={mapRef} className="border border-rule bg-bg p-3 overflow-x-auto min-h-0 self-start">
        <div className="min-w-[760px]">{map}</div>
      </div>
      <div
        className="@5xl:sticky @5xl:top-16 min-h-0 overflow-hidden"
        style={pairedLayout && mapHeight ? { height: mapHeight } : undefined}
      >
        {datasheet({
          className: pairedLayout ? "h-full" : undefined,
          layoutKey: pairedLayout && mapHeight !== undefined ? mapHeight : "stack",
        })}
      </div>
    </div>
  )
}
