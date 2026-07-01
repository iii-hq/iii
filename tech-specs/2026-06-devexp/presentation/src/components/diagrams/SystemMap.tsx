import { useMemo } from 'react'
import { FnChip } from '@/components/schematic/FnChip'
import {
  KIND_HUE,
  MAP_EDGES,
  MAP_NODES,
  type MapNode,
} from '@/content/map'
import { cn } from '@/lib/utils'

interface SystemMapProps {
  selected: string
  onSelect: (id: string) => void
}

const node = (id: string): MapNode => MAP_NODES.find((n) => n.id === id)!

export function SystemMap({ selected, onSelect }: SystemMapProps) {
  const reducedMotion = useMemo(
    () =>
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    [],
  )

  const activeEdges = useMemo(
    () =>
      new Set(
        MAP_EDGES.filter((e) => e.from === selected || e.to === selected).map(
          (e) => e.id,
        ),
      ),
    [selected],
  )
  const connected = useMemo(() => {
    const set = new Set<string>([selected])
    for (const e of MAP_EDGES) {
      if (e.from === selected) set.add(e.to)
      if (e.to === selected) set.add(e.from)
    }
    return set
  }, [selected])

  return (
    <svg
      viewBox="0 0 1080 700"
      role="group"
      aria-label="the four-planes system map"
      className="w-full h-auto font-mono select-none"
    >
      <defs>
        <marker id="m-arr-faint" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
          <path d="M 0 0 L 8 4 L 0 8 z" className="fill-ink-ghost" />
        </marker>
        <marker id="m-arr-accent" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
          <path d="M 0 0 L 8 4 L 0 8 z" className="fill-accent" />
        </marker>
      </defs>

      {/* edges under nodes */}
      {MAP_EDGES.map((edge) => {
        const active = activeEdges.has(edge.id)
        return (
          <g key={edge.id}>
            <path
              d={edge.d}
              fill="none"
              strokeWidth={active ? 1.5 : 1}
              strokeDasharray={edge.dashed ? '5 4' : undefined}
              markerEnd={`url(#${active ? 'm-arr-accent' : 'm-arr-faint'})`}
              markerStart={edge.bidir ? `url(#${active ? 'm-arr-accent' : 'm-arr-faint'})` : undefined}
              className={cn(
                'transition-[stroke] duration-200',
                active ? 'stroke-accent' : 'stroke-rule',
              )}
            />
            {active && !reducedMotion ? (
              <circle r="2.6" className="fill-accent">
                <animateMotion dur={`${edge.dur ?? 2}s`} repeatCount="indefinite" path={edge.d} />
              </circle>
            ) : null}
            {edge.meeting ? (
              <g>
                <circle
                  cx={edge.lx - 10}
                  cy={edge.ly - 4}
                  r="7.5"
                  className={cn(active ? 'fill-accent' : 'fill-bg', active ? 'stroke-accent' : 'stroke-ink-faint')}
                  strokeWidth={1}
                />
                <text
                  x={edge.lx - 10}
                  y={edge.ly - 0.5}
                  textAnchor="middle"
                  fontSize="9"
                  className={active ? 'fill-bg' : 'fill-ink-faint'}
                >
                  {edge.meeting}
                </text>
              </g>
            ) : null}
            <text
              x={edge.lx}
              y={edge.ly}
              textAnchor={edge.anchor ?? 'middle'}
              fontSize="9.5"
              letterSpacing="0.03em"
              className={cn(active ? 'fill-ink' : 'fill-ink-ghost', 'transition-[fill] duration-200')}
              style={{ paintOrder: 'stroke', stroke: 'var(--color-bg)', strokeWidth: 4 }}
            >
              {edge.label}
            </text>
          </g>
        )
      })}

      {/* nodes */}
      {MAP_NODES.map((n) => {
        const isSelected = n.id === selected
        const isConnected = connected.has(n.id)
        const dashed = n.id === 'w3'
        return (
          <g
            key={n.id}
            role="button"
            tabIndex={0}
            aria-pressed={isSelected}
            aria-label={`select ${n.title}`}
            onClick={() => onSelect(n.id)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault()
                onSelect(n.id)
              }
            }}
            className="cursor-pointer focus:outline-none group"
          >
            <rect
              x={n.x}
              y={n.y}
              width={n.w}
              height={n.h}
              strokeWidth={isSelected ? 1.5 : n.kind === 'wk' || n.kind === 'file' ? 1 : 1.25}
              strokeDasharray={dashed ? '5 4' : undefined}
              className={cn(
                'transition-all duration-200',
                isSelected
                  ? 'fill-panel stroke-accent'
                  : 'fill-bg group-hover:fill-panel',
                !isSelected && (isConnected ? 'stroke-ink-faint' : 'stroke-rule'),
              )}
            />
            {isSelected ? (
              <rect x={n.x} y={n.y} width={3} height={n.h} className="fill-accent" />
            ) : null}

            {/* kind dot — the one rationed splash of plane-hue */}
            <circle cx={n.x + 14} cy={n.y + 15} r="3.5" style={{ fill: KIND_HUE[n.kind] }} />

            <text
              x={n.x + n.w - 9}
              y={n.y + 18}
              textAnchor="end"
              fontSize="7.5"
              letterSpacing="0.1em"
              className={cn('uppercase', isSelected ? 'fill-accent' : 'fill-ink-ghost')}
            >
              {n.kindLabel}
            </text>
            <text
              x={n.x + 26}
              y={n.y + n.h / 2 + 4}
              fontSize={n.kind === 'file' || n.kind === 'wk' ? '13.5' : '15'}
              fontWeight={600}
              className={cn(
                isSelected || isConnected || n.kind === 'brain' || n.kind === 'pid'
                  ? 'fill-ink'
                  : 'fill-ink-faint',
              )}
            >
              {n.title}
            </text>
            <text
              x={n.x + 26}
              y={n.y + n.h / 2 + 19}
              fontSize="9.5"
              letterSpacing="0.03em"
              className="fill-ink-ghost"
            >
              {n.sub}
            </text>
          </g>
        )
      })}
    </svg>
  )
}

const SHEET_ROWS: Array<{ key: keyof MapNode['sheet']; label: string }> = [
  { key: 'owns', label: 'owns' },
  { key: 'neverDoes', label: 'never does' },
  { key: 'functions', label: 'functions' },
]

export function MapDatasheet({
  selected,
  className,
}: {
  selected: string
  className?: string
}) {
  const n = node(selected)
  return (
    <aside
      className={cn('border border-rule bg-bg flex flex-col min-w-0', className)}
    >
      <header className="shrink-0 flex items-center justify-between gap-x-3 bg-panel px-4 py-3 border-b border-rule">
        <span className="flex items-center gap-x-2 min-w-0">
          <span
            aria-hidden
            className="inline-block size-2 rounded-full shrink-0"
            style={{ background: KIND_HUE[n.kind] }}
          />
          <span className="font-mono text-[15px] font-semibold text-ink truncate">{n.title}</span>
        </span>
        <span className="font-mono text-[10px] uppercase tracking-[0.08em] text-ink-faint whitespace-nowrap shrink-0">
          {n.kindLabel}
        </span>
      </header>

      <div className="px-4 py-3.5 font-mono text-[13px] leading-[1.7] text-ink lowercase border-b border-rule-2">
        {n.sheet.role}
      </div>

      {SHEET_ROWS.map((row) => (
        <div key={row.key} className="px-4 py-3 border-b border-rule-2 last:border-b-0">
          <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint mb-1.5">
            {row.label}
          </div>
          <div className="font-mono text-[12px] leading-[1.65] text-ink-faint lowercase">
            {n.sheet[row.key]}
          </div>
        </div>
      ))}

      <div className="shrink-0 bg-panel border-t border-rule px-4 py-2.5">
        <FnChip tone="faint" className="max-w-full whitespace-normal">
          {n.sheet.install}
        </FnChip>
      </div>
    </aside>
  )
}
