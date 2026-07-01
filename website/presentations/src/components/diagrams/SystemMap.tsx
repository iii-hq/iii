import { FnChip } from '@lib/components/schematic/FnChip'
import { Prompt } from '@lib/components/schematic/Prompt'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { cn } from '@lib/lib/utils'
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react'

/**
 * A1 / A4 — the system map archetype.
 *
 * A data-driven SVG topology: rectangles for nodes, hand-authored Bezier paths
 * for edges. Selecting a node highlights the edges that touch it (accent +
 * travelling pulse) and surfaces its datasheet. Everything is props — supply a
 * `nodes` + `edges` description and a record of `MapNodeInfo` and the component
 * does the rest. Hand-author the edge `d` paths against the same coordinate
 * space as the node x/y/w/h (default 1030×600 viewBox).
 */

export type MapNodeKind = 'primary' | 'secondary' | 'optional' | 'external'

export interface MapNode {
  id: string
  x: number
  y: number
  w: number
  h: number
  title: string
  sub?: string
  kind: MapNodeKind
  /** override the small uppercase corner tag; defaults from kind */
  tag?: string
}

export interface MapEdge {
  id: string
  from: string
  to: string
  /** SVG path in the same coordinate space as the nodes */
  d: string
  label?: string
  /** label anchor point */
  lx?: number
  ly?: number
  anchor?: 'start' | 'middle' | 'end'
  dashed?: boolean
  /** seconds for the travelling pulse on the active edge */
  dur?: number
}

const KIND_TAG: Record<MapNodeKind, string> = {
  primary: 'core',
  secondary: 'worker',
  optional: 'optional',
  external: 'external',
}

interface SystemMapProps {
  nodes: MapNode[]
  edges: MapEdge[]
  selected: string
  onSelect: (id: string) => void
  /** SVG coordinate space; author node + edge coordinates to match */
  width?: number
  height?: number
  className?: string
}

export function SystemMap({ nodes, edges, selected, onSelect, width = 1030, height = 600, className }: SystemMapProps) {
  const reducedMotion = useMemo(
    () => typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    [],
  )

  const activeEdges = useMemo(
    () => new Set(edges.filter((e) => e.from === selected || e.to === selected).map((e) => e.id)),
    [edges, selected],
  )
  const connected = useMemo(() => {
    const set = new Set<string>([selected])
    for (const e of edges) {
      if (e.from === selected) set.add(e.to)
      if (e.to === selected) set.add(e.from)
    }
    return set
  }, [edges, selected])

  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      role="group"
      aria-label="system map"
      className={cn('w-full h-auto font-mono select-none', className)}
    >
      <defs>
        <marker
          id="map-arr-faint"
          viewBox="0 0 8 8"
          refX="7"
          refY="4"
          markerWidth="7"
          markerHeight="7"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 8 4 L 0 8 z" className="fill-ink-ghost" />
        </marker>
        <marker
          id="map-arr-accent"
          viewBox="0 0 8 8"
          refX="7"
          refY="4"
          markerWidth="7"
          markerHeight="7"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 8 4 L 0 8 z" className="fill-accent" />
        </marker>
      </defs>

      {/* edges under nodes */}
      {edges.map((edge) => {
        const active = activeEdges.has(edge.id)
        return (
          <g key={edge.id}>
            <path
              d={edge.d}
              fill="none"
              strokeWidth={active ? 1.4 : 1}
              strokeDasharray={edge.dashed ? '5 4' : undefined}
              markerEnd={`url(#${active ? 'map-arr-accent' : 'map-arr-faint'})`}
              className={cn('transition-[stroke] duration-200', active ? 'stroke-accent' : 'stroke-rule')}
            />
            {active && !reducedMotion ? (
              <circle r="2.6" className="fill-accent">
                <animateMotion dur={`${edge.dur ?? 2}s`} repeatCount="indefinite" path={edge.d} />
              </circle>
            ) : null}
            {edge.label ? (
              <text
                x={edge.lx ?? 0}
                y={edge.ly ?? 0}
                textAnchor={edge.anchor ?? 'middle'}
                fontSize="9.5"
                letterSpacing="0.04em"
                className={cn(active ? 'fill-ink' : 'fill-ink-ghost', 'transition-[fill] duration-200')}
                style={{
                  paintOrder: 'stroke',
                  stroke: 'var(--color-bg)',
                  strokeWidth: 4,
                }}
              >
                {edge.label}
              </text>
            ) : null}
          </g>
        )
      })}

      {/* nodes */}
      {nodes.map((node) => {
        const isSelected = node.id === selected
        const isConnected = connected.has(node.id)
        const strong = node.kind === 'primary'
        return (
          <g
            key={node.id}
            role="button"
            tabIndex={0}
            aria-pressed={isSelected}
            aria-label={`select ${node.title}`}
            onClick={() => onSelect(node.id)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault()
                onSelect(node.id)
              }
            }}
            className="cursor-pointer focus:outline-none group"
          >
            <rect
              x={node.x}
              y={node.y}
              width={node.w}
              height={node.h}
              strokeWidth={isSelected ? 1.5 : strong ? 1.25 : 1}
              strokeDasharray={node.kind === 'optional' ? '5 4' : undefined}
              className={cn(
                'transition-all duration-200',
                isSelected ? 'fill-panel stroke-accent' : 'fill-bg group-hover:fill-panel',
                !isSelected &&
                  (strong
                    ? isConnected
                      ? 'stroke-ink'
                      : 'stroke-ink-ghost'
                    : isConnected
                      ? 'stroke-ink-faint'
                      : 'stroke-rule'),
              )}
            />
            {isSelected ? <rect x={node.x} y={node.y} width={3} height={node.h} className="fill-accent" /> : null}
            <text
              x={node.x + node.w / 2}
              y={node.y + node.h / 2 + (node.sub ? -4 : 4)}
              textAnchor="middle"
              fontSize="14"
              fontWeight={600}
              className={cn(isSelected || isConnected || strong ? 'fill-ink' : 'fill-ink-faint')}
            >
              {node.title}
            </text>
            {node.sub ? (
              <text
                x={node.x + node.w / 2}
                y={node.y + node.h / 2 + 13}
                textAnchor="middle"
                fontSize="9.5"
                letterSpacing="0.05em"
                className="fill-ink-ghost"
              >
                {node.sub}
              </text>
            ) : null}
            <text
              x={node.x + node.w - 7}
              y={node.y + 13}
              textAnchor="end"
              fontSize="8"
              letterSpacing="0.08em"
              className={cn('uppercase', isSelected ? 'fill-accent' : 'fill-ink-ghost')}
            >
              {node.tag ?? KIND_TAG[node.kind]}
            </text>
          </g>
        )
      })}
    </svg>
  )
}

/* ---- datasheet ---- */

export interface InfoItem {
  name: string
  desc?: string
}

export interface InfoSection {
  /** small caps heading, e.g. "surface" or "emits" */
  heading: string
  items: InfoItem[]
  /** render items with a leading status dot (good for events) */
  dotted?: boolean
}

export interface MapNodeInfo {
  id: string
  /** small caps tag in the header, e.g. "core worker" */
  kindLabel: string
  /** one-line role summary at the top */
  role: string
  sections?: InfoSection[]
  /** closing faint paragraph */
  note?: string
  /** left-border caveat lines */
  bullets?: string[]
  /** bottom command chip */
  install?: string
}

function ScrollFadePanel({
  children,
  contentKey,
  layoutKey,
}: {
  children: ReactNode
  contentKey: string
  layoutKey?: string | number
}) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const [canScrollUp, setCanScrollUp] = useState(false)
  const [canScrollDown, setCanScrollDown] = useState(false)

  const updateScrollState = useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    const { scrollTop, scrollHeight, clientHeight } = el
    const overflow = scrollHeight - clientHeight > 8
    setCanScrollUp(overflow && scrollTop > 4)
    setCanScrollDown(overflow && scrollTop + clientHeight < scrollHeight - 4)
  }, [])

  useEffect(() => {
    const scrollEl = scrollRef.current
    const contentEl = contentRef.current
    if (!scrollEl || !contentEl) return

    scrollEl.scrollTop = 0

    const sync = () => {
      updateScrollState()
    }

    sync()
    requestAnimationFrame(sync)

    scrollEl.addEventListener('scroll', sync, { passive: true })
    const observer = new ResizeObserver(sync)
    observer.observe(scrollEl)
    observer.observe(contentEl)

    return () => {
      scrollEl.removeEventListener('scroll', sync)
      observer.disconnect()
    }
  }, [contentKey, layoutKey, updateScrollState])

  return (
    <div className="relative min-h-0 flex-1 overflow-hidden">
      <div ref={scrollRef} className="h-full overflow-y-auto overscroll-contain [scrollbar-gutter:stable]">
        <div ref={contentRef}>{children}</div>
      </div>
      {canScrollUp ? (
        <div
          aria-hidden
          className="pointer-events-none absolute inset-x-0 top-0 z-10 h-8 border-b border-rule bg-gradient-to-b from-bg via-bg/95 to-transparent"
        />
      ) : null}
      {canScrollDown ? (
        <>
          <div
            aria-hidden
            className="pointer-events-none absolute inset-x-0 bottom-0 z-10 h-12 bg-gradient-to-t from-bg via-bg/95 to-transparent"
          />
          <div
            aria-hidden
            className="pointer-events-none absolute inset-x-0 bottom-0 z-10 flex items-end justify-center gap-x-1.5 pb-2"
          >
            <span className="font-mono text-[9px] uppercase tracking-[0.14em] text-accent">scroll</span>
            <span className="font-mono text-[10px] leading-none text-accent">↓</span>
          </div>
        </>
      ) : null}
    </div>
  )
}

/** the side datasheet for the selected map node */
export function MapDatasheet({
  info,
  className,
  layoutKey,
}: {
  info: MapNodeInfo
  className?: string
  layoutKey?: string | number
}) {
  return (
    <aside className={cn('border border-rule bg-bg flex flex-col min-w-0 min-h-0 overflow-hidden', className)}>
      <header className="shrink-0 flex items-center justify-between gap-x-3 bg-panel px-4 py-3 border-b border-rule">
        <span className="font-mono text-[16px] font-semibold text-ink">{info.id}</span>
        <span className="font-mono text-[10px] uppercase tracking-[0.08em] text-ink-faint whitespace-nowrap">
          {info.kindLabel}
        </span>
      </header>

      <ScrollFadePanel contentKey={info.id} layoutKey={layoutKey}>
        <div className="px-4 py-3.5 font-mono text-[13px] leading-[1.7] text-ink lowercase border-b border-rule-2">
          {info.role}
        </div>

        {(info.sections ?? []).map((section) => (
          <div key={section.heading} className="px-4 py-3.5 border-b border-rule-2">
            <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint mb-2.5">
              {section.heading}
            </div>
            <ul className="flex flex-col gap-y-2.5">
              {section.items.map((item) => (
                <li key={item.name} className="min-w-0">
                  {section.dotted ? (
                    <span className="flex items-baseline gap-x-2 min-w-0">
                      <StatusDot tone="accent" className="translate-y-[-1px]" />
                      <span className="min-w-0">
                        <span className="font-mono text-[11.5px] text-ink">{item.name}</span>
                        {item.desc ? (
                          <span className="font-mono text-[11.5px] text-ink-faint lowercase"> — {item.desc}</span>
                        ) : null}
                      </span>
                    </span>
                  ) : (
                    <>
                      <FnChip tone="ink" className="max-w-full overflow-hidden">
                        {item.name}
                      </FnChip>
                      {item.desc ? (
                        <div className="mt-1 font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase">
                          {item.desc}
                        </div>
                      ) : null}
                    </>
                  )}
                </li>
              ))}
            </ul>
          </div>
        ))}

        {info.note ? (
          <div className="px-4 py-3.5 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase">{info.note}</div>
        ) : null}

        {info.bullets && info.bullets.length > 0 ? (
          <div className="px-4 pb-3.5 flex flex-col gap-y-1.5">
            {info.bullets.map((note) => (
              <div
                key={note}
                className="border-l-2 border-rule pl-2.5 font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase"
              >
                {note}
              </div>
            ))}
          </div>
        ) : null}
      </ScrollFadePanel>

      {info.install ? (
        <div className="shrink-0 bg-panel border-t border-rule px-4 py-2.5 font-mono text-[12.5px]">
          <Prompt symbol="$">{info.install}</Prompt>
        </div>
      ) : null}
    </aside>
  )
}
