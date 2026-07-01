import { FnChip } from '@lib/components/schematic/FnChip'
import { Prompt } from '@lib/components/schematic/Prompt'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { cn } from '@lib/lib/utils'
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { WORKERS } from '../content/workers'

type NodeKind = 'core' | 'consumer' | 'sibling'

interface MapNode {
  id: string
  x: number
  y: number
  w: number
  h: number
  title: string
  sub: string
  kind: NodeKind
}

interface MapEdge {
  id: string
  from: string
  to: string
  d: string
  label: string
  lx: number
  ly: number
  anchor?: 'start' | 'middle' | 'end'
  dashed?: boolean
  dur?: number
}

const NODES: MapNode[] = [
  { id: 'chat', x: 30, y: 60, w: 180, h: 56, title: 'chat', sub: 'console web app', kind: 'consumer' },
  { id: 'telegram-bot', x: 30, y: 185, w: 180, h: 56, title: 'telegram-bot', sub: 'webhook bridge', kind: 'consumer' },
  { id: 'third-party', x: 30, y: 310, w: 180, h: 56, title: 'third-party-worker', sub: 'any worker', kind: 'consumer' },
  { id: 'session-manager', x: 425, y: 28, w: 210, h: 60, title: 'session-manager', sub: 'session::*', kind: 'core' },
  { id: 'harness', x: 425, y: 168, w: 210, h: 92, title: 'harness', sub: 'harness::* — the loop', kind: 'core' },
  { id: 'context-manager', x: 425, y: 330, w: 210, h: 60, title: 'context-manager', sub: 'context::*', kind: 'core' },
  { id: 'approval-gate', x: 800, y: 28, w: 200, h: 60, title: 'approval-gate', sub: 'approval::*', kind: 'sibling' },
  { id: 'llm-router', x: 800, y: 168, w: 200, h: 60, title: 'llm-router', sub: 'router::*', kind: 'core' },
]

const EDGES: MapEdge[] = [
  {
    id: 'chat-send',
    from: 'chat',
    to: 'harness',
    d: 'M 210 92 C 300 92, 340 196, 425 196',
    label: 'harness::send',
    lx: 308,
    ly: 132,
    dur: 2.2,
  },
  {
    id: 'session-events',
    from: 'session-manager',
    to: 'chat',
    d: 'M 425 50 C 340 50, 300 76, 214 84',
    label: 'live session events',
    lx: 318,
    ly: 42,
    dur: 2.2,
  },
  {
    id: 'tg-send',
    from: 'telegram-bot',
    to: 'harness',
    d: 'M 210 213 C 300 213, 340 214, 425 214',
    label: 'harness::send',
    lx: 304,
    ly: 206,
    dur: 2.2,
  },
  {
    id: 'persist',
    from: 'harness',
    to: 'session-manager',
    d: 'M 460 168 L 460 92',
    label: 'append / stream deltas',
    lx: 452,
    ly: 136,
    anchor: 'end',
    dur: 1.6,
  },
  {
    id: 'assemble',
    from: 'harness',
    to: 'context-manager',
    d: 'M 540 260 L 540 326',
    label: 'context::assemble',
    lx: 548,
    ly: 298,
    anchor: 'start',
    dur: 1.6,
  },
  {
    id: 'generate',
    from: 'harness',
    to: 'llm-router',
    d: 'M 635 200 L 796 200',
    label: 'router::chat',
    lx: 712,
    ly: 192,
    dur: 1.8,
  },
  {
    id: 'providers',
    from: 'llm-router',
    to: 'providers',
    d: 'M 900 228 L 900 296',
    label: 'provider::<id>::stream',
    lx: 908,
    ly: 266,
    anchor: 'start',
    dur: 1.6,
  },
  {
    id: 'hook',
    from: 'harness',
    to: 'approval-gate',
    d: 'M 638 176 C 716 142, 750 84, 796 60',
    label: 'hook: pre_trigger',
    lx: 712,
    ly: 94,
    dashed: true,
    dur: 2.2,
  },
  {
    id: 'resolve',
    from: 'approval-gate',
    to: 'harness',
    d: 'M 798 76 C 730 102, 690 142, 640 188',
    label: 'function::resolve',
    lx: 758,
    ly: 134,
    anchor: 'start',
    dashed: true,
    dur: 2.2,
  },
  {
    id: 'dispatch',
    from: 'harness',
    to: 'substrate',
    d: 'M 455 260 C 400 320, 340 400, 300 458',
    label: 'agent_trigger → any allowed function',
    lx: 312,
    ly: 392,
    anchor: 'start',
    dur: 2.4,
  },
  {
    id: 'callback',
    from: 'substrate',
    to: 'chat',
    d: 'M 250 462 L 250 134 C 250 116, 240 104, 214 98',
    label: 'functions can call consumers back',
    lx: 258,
    ly: 286,
    anchor: 'start',
    dur: 3,
  },
  {
    id: 'run',
    from: 'third-party',
    to: 'harness',
    d: 'M 210 326 C 300 318, 350 250, 425 238',
    label: 'harness::run',
    lx: 296,
    ly: 288,
    dur: 2.2,
  },
  {
    id: 'direct',
    from: 'third-party',
    to: 'llm-router',
    d: 'M 210 352 C 460 450, 720 410, 798 212',
    label: 'router::chat — no loop needed',
    lx: 560,
    ly: 428,
    dur: 3,
  },
  {
    id: 'spawn',
    from: 'harness',
    to: 'harness',
    d: 'M 520 168 C 520 118, 620 118, 620 168',
    label: 'harness::spawn → child sessions',
    lx: 575,
    ly: 110,
    dur: 2.4,
  },
]

const SUBSTRATE_CHIPS: Array<{ id: string; ghost?: boolean }> = [
  { id: 'shell::exec' },
  { id: 'email::send' },
  { id: 'database::query' },
  { id: 'storage::put' },
  { id: 'image-resize::convert' },
  { id: 'todo::create' },
  { id: 'coder::apply_patch' },
  { id: 'search::web' },
  { id: 'engine::functions::list' },
  { id: 'harness::spawn' },
  { id: 'anything::you_register', ghost: true },
]

const SUBSTRATE = { x: 30, y: 462, w: 970, h: 108 }

function chipWidth(label: string) {
  return Math.round(label.length * 6.6) + 18
}

interface SystemMapProps {
  selected: string
  onSelect: (id: string) => void
}

export function SystemMap({ selected, onSelect }: SystemMapProps) {
  const reducedMotion = useMemo(
    () => typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    [],
  )

  const activeEdges = useMemo(
    () => new Set(EDGES.filter((e) => e.from === selected || e.to === selected).map((e) => e.id)),
    [selected],
  )
  const connected = useMemo(() => {
    const set = new Set<string>([selected])
    for (const e of EDGES) {
      if (e.from === selected) set.add(e.to)
      if (e.to === selected) set.add(e.from)
    }
    return set
  }, [selected])

  // substrate chip layout: two rows, wrapped manually
  const chipRows = useMemo(() => {
    const rows: Array<Array<{ id: string; ghost?: boolean; x: number; w: number }>> = [[], []]
    let x = SUBSTRATE.x + 18
    let row = 0
    for (const chip of SUBSTRATE_CHIPS) {
      const w = chipWidth(chip.id)
      if (x + w > SUBSTRATE.x + SUBSTRATE.w - 18 && row === 0) {
        row = 1
        x = SUBSTRATE.x + 18
      }
      rows[row].push({ ...chip, x, w })
      x += w + 8
    }
    return rows
  }, [])

  const substrateActive = selected === 'substrate'

  return (
    <svg
      viewBox="0 0 1030 600"
      role="group"
      aria-label="system map of the agentic workers"
      className="w-full h-auto font-mono select-none"
    >
      <defs>
        <marker
          id="arr-faint"
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
          id="arr-accent"
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
      {EDGES.map((edge) => {
        const active = activeEdges.has(edge.id)
        return (
          <g key={edge.id}>
            <path
              d={edge.d}
              fill="none"
              strokeWidth={active ? 1.4 : 1}
              strokeDasharray={edge.dashed ? '5 4' : undefined}
              markerEnd={`url(#${active ? 'arr-accent' : 'arr-faint'})`}
              className={cn('transition-[stroke] duration-200', active ? 'stroke-accent' : 'stroke-rule')}
            />
            {active && !reducedMotion ? (
              <circle r="2.6" className="fill-accent">
                <animateMotion dur={`${edge.dur ?? 2}s`} repeatCount="indefinite" path={edge.d} />
              </circle>
            ) : null}
            <text
              x={edge.lx}
              y={edge.ly}
              textAnchor={edge.anchor ?? 'middle'}
              fontSize="9.5"
              letterSpacing="0.04em"
              className={cn(active ? 'fill-ink' : 'fill-ink-ghost', 'transition-[fill] duration-200')}
              style={{ paintOrder: 'stroke', stroke: 'var(--color-bg)', strokeWidth: 4 }}
            >
              {edge.label}
            </text>
          </g>
        )
      })}

      {/* worker / consumer nodes */}
      {NODES.map((node) => {
        const isSelected = node.id === selected
        const isConnected = connected.has(node.id)
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
              strokeWidth={isSelected ? 1.5 : node.kind === 'core' ? 1.25 : 1}
              strokeDasharray={node.kind === 'sibling' ? '5 4' : undefined}
              className={cn(
                'transition-all duration-200',
                isSelected ? 'fill-panel stroke-accent' : 'fill-bg group-hover:fill-panel',
                !isSelected &&
                  (node.kind === 'core'
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
              y={node.y + node.h / 2 - 4}
              textAnchor="middle"
              fontSize="14"
              fontWeight={600}
              className={cn(isSelected || isConnected || node.kind === 'core' ? 'fill-ink' : 'fill-ink-faint')}
            >
              {node.title}
            </text>
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
            <text
              x={node.x + node.w - 7}
              y={node.y + 13}
              textAnchor="end"
              fontSize="8"
              letterSpacing="0.08em"
              className={cn('uppercase', isSelected ? 'fill-accent' : 'fill-ink-ghost')}
            >
              {node.kind === 'core' ? 'worker' : node.kind === 'sibling' ? 'optional' : 'consumer'}
            </text>
          </g>
        )
      })}

      {/* provider stack (ghost — llm-router's protocol side) */}
      <g
        role="button"
        tabIndex={0}
        aria-label="select provider workers"
        onClick={() => onSelect('llm-router')}
        className="cursor-pointer"
      >
        {[0, 1, 2].map((i) => (
          <rect
            key={i}
            x={800 + i * 5}
            y={300 + i * 5}
            width={190}
            height={62}
            className={cn(
              'fill-bg transition-colors',
              connected.has('providers') || selected === 'llm-router' ? 'stroke-ink-faint' : 'stroke-rule',
            )}
            strokeWidth={1}
          />
        ))}
        <text x={905} y={332} textAnchor="middle" fontSize="12.5" fontWeight={600} className="fill-ink-faint">
          provider workers
        </text>
        <text x={905} y={349} textAnchor="middle" fontSize="9" letterSpacing="0.05em" className="fill-ink-ghost">
          anthropic / openai / google / …
        </text>
      </g>

      {/* the substrate band */}
      <g
        role="button"
        tabIndex={0}
        aria-pressed={substrateActive}
        aria-label="select the substrate"
        onClick={() => onSelect('substrate')}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            onSelect('substrate')
          }
        }}
        className="cursor-pointer focus:outline-none group"
      >
        <rect
          x={SUBSTRATE.x}
          y={SUBSTRATE.y}
          width={SUBSTRATE.w}
          height={SUBSTRATE.h}
          strokeWidth={substrateActive ? 1.5 : 1}
          className={cn(
            'transition-all duration-200',
            substrateActive ? 'fill-panel stroke-accent' : 'fill-paper-2 stroke-rule group-hover:stroke-ink-faint',
          )}
        />
        <text
          x={SUBSTRATE.x + 18}
          y={SUBSTRATE.y + 22}
          fontSize="10"
          letterSpacing="0.14em"
          className={cn('uppercase', substrateActive ? 'fill-accent' : 'fill-ink-faint')}
        >
          the substrate — every function registered on the bus
        </text>
        <text
          x={SUBSTRATE.x + SUBSTRATE.w - 18}
          y={SUBSTRATE.y + 22}
          textAnchor="end"
          fontSize="9"
          letterSpacing="0.08em"
          className="fill-ink-ghost uppercase"
        >
          deny by default — allow-lists opt functions in
        </text>
        {chipRows.map((row, rowIdx) =>
          row.map((chip) => (
            <g key={chip.id}>
              <rect
                x={chip.x}
                y={SUBSTRATE.y + 36 + rowIdx * 30}
                width={chip.w}
                height={22}
                className={cn('fill-bg', chip.ghost ? 'stroke-rule-2' : 'stroke-rule')}
                strokeWidth={1}
              />
              <text
                x={chip.x + chip.w / 2}
                y={SUBSTRATE.y + 36 + rowIdx * 30 + 15}
                textAnchor="middle"
                fontSize="10"
                className={chip.ghost ? 'fill-ink-ghost' : 'fill-ink-faint'}
              >
                {chip.id}
              </text>
            </g>
          )),
        )}
      </g>
    </svg>
  )
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
  selected,
  className,
  layoutKey,
}: {
  selected: string
  className?: string
  layoutKey?: string | number
}) {
  const info = WORKERS[selected] ?? WORKERS.harness
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

        <div className="px-4 py-3.5 border-b border-rule-2">
          <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint mb-2.5">surface</div>
          <ul className="flex flex-col gap-y-2.5">
            {info.functions.map((fn) => (
              <li key={fn.id} className="min-w-0">
                <FnChip tone="ink" className="max-w-full overflow-hidden">
                  {fn.id}
                </FnChip>
                <div className="mt-1 font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase">{fn.desc}</div>
              </li>
            ))}
          </ul>
        </div>

        {info.emits && info.emits.length > 0 ? (
          <div className="px-4 py-3.5 border-b border-rule-2">
            <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint mb-2.5">
              emits — bind these, render live
            </div>
            <ul className="flex flex-col gap-y-1.5">
              {info.emits.map((evt) => (
                <li key={evt.id} className="flex items-baseline gap-x-2 min-w-0">
                  <StatusDot tone="accent" className="translate-y-[-1px]" />
                  <div className="min-w-0">
                    <span className="font-mono text-[11.5px] text-ink">{evt.id}</span>
                    <span className="font-mono text-[11.5px] text-ink-faint lowercase"> — {evt.desc}</span>
                  </div>
                </li>
              ))}
            </ul>
          </div>
        ) : null}

        <div className="px-4 py-3.5 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase">
          {info.standalone}
        </div>

        {info.notes && info.notes.length > 0 ? (
          <div className="px-4 pb-3.5 flex flex-col gap-y-1.5">
            {info.notes.map((note) => (
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
