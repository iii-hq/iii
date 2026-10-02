'use client'

import { AnimatePresence, motion } from 'motion/react'
import { type CSSProperties, useId, useState } from 'react'

import { DemoPlayback } from '@/components/graphics/demo-playback'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import type { CodeLine } from '@/lib/highlight'
import { easeOut, spring } from '@/lib/motion'
import { cn } from '@/lib/utils'

import { demo } from './content'
import { HOPS, type Hop, workerById } from './graph/model'
import { GraphScroller } from './graph/scroller'
import { SystemGraph } from './graph/system-graph'
import { useStageClock } from './graph/use-stage-clock'
import { demoCode, demoLogs } from './live-demo-code'

type View = (typeof demo.views)[number]

const TOTAL_MS = HOPS.reduce((sum, h) => sum + h.ms, 0)
/** Trace bars share one timeline; each hop starts where the previous one ended. */
const STARTS = HOPS.map((_, i) => HOPS.slice(0, i).reduce((sum, h) => sum + h.ms, 0))

export function LiveDemoClient({ code }: { code: CodeLine[] }) {
  const id = useId()
  const { ref, running, paused, setPaused, reduce } = useDemoPlayback<HTMLDivElement>()
  const { step, cycle } = useStageClock('execution', running)
  const [view, setView] = useState<View>('Graph')
  const [pinned, setPinned] = useState<string | null>(null)

  const hop: Hop | null = step < HOPS.length ? HOPS[step] : null
  const done = step >= HOPS.length
  const detailHop = pinned ? (HOPS.find((h) => h.node === pinned) ?? HOPS[0]) : (hop ?? HOPS[HOPS.length - 1])
  const caption = hop
    ? `${hop.fn} · ${hop.lang} · ${hop.where} · ${hop.ms.toLocaleString('en-US')}ms`
    : `done · github::pr::watch registered · ${TOTAL_MS.toLocaleString('en-US')}ms end to end`

  return (
    <div ref={ref} className="mt-10 lg:mt-14">
      <div className="flex flex-col gap-4 border-b pb-4 sm:flex-row sm:items-center sm:justify-between">
        <p className="flex min-w-0 items-center gap-2.5 font-sans text-[13px] text-muted-foreground">
          <span aria-hidden className="shrink-0 text-foreground">
            ›
          </span>
          <span className="truncate text-foreground">{demo.prompt}</span>
        </p>
        <div className="flex items-center gap-2">
          <div role="tablist" aria-label="Demo view" className="relative flex rounded-lg border bg-card p-0.5">
            {demo.views.map((v) => (
              <button
                key={v}
                type="button"
                role="tab"
                aria-selected={view === v}
                aria-controls={`${id}-panel`}
                onClick={() => setView(v)}
                className={cn(
                  'relative z-10 h-8 rounded-md px-3.5 font-medium text-[13px] outline-none transition-colors focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2',
                  view === v ? 'text-foreground' : 'text-muted-foreground hover:text-foreground',
                )}
              >
                {view === v ? (
                  <motion.span
                    layoutId={`${id}-view`}
                    aria-hidden
                    className="absolute inset-0 -z-10 rounded-md bg-faint shadow-[inset_0_0_0_1px_var(--line)]"
                    transition={spring.snappy}
                  />
                ) : null}
                {v}
              </button>
            ))}
          </div>
          <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
        </div>
      </div>

      <div id={`${id}-panel`} role="tabpanel" className="graphic-stage mt-6">
        {view === 'Graph' ? (
          <div className="swap-in grid grid-cols-[minmax(0,1fr)] gap-6 lg:grid-cols-[minmax(0,1fr)_300px] lg:gap-10">
            <GraphScroller className="min-w-0">
              <SystemGraph
                stage="execution"
                step={step}
                cycle={cycle}
                active={running}
                selected={pinned}
                onSelect={(node) => setPinned((p) => (p === node ? null : node))}
                label="A request enters the harness. agent::run calls browser::navigate, browser::act, extract::text, embed::vectors, pg::query, llm::complete and github::pr::watch in turn. Every call travels through the iii engine, and each worker lights up as it executes."
              />
            </GraphScroller>
            <DetailPanel
              hop={detailHop}
              live={!pinned}
              current={hop?.node === detailHop.node}
              onFollow={() => setPinned(null)}
            />
          </div>
        ) : view === 'Code' ? (
          <CodeView code={code} line={hop?.code ?? null} done={done} />
        ) : (
          <TraceView step={step} />
        )}
      </div>

      <div
        aria-hidden
        className="mt-5 flex h-5 items-center justify-center overflow-hidden text-center font-sans text-[13px] text-muted-foreground"
      >
        <AnimatePresence mode="wait" initial={false}>
          <motion.span
            key={caption}
            className="truncate"
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.3, ease: easeOut }}
          >
            {caption}
          </motion.span>
        </AnimatePresence>
      </div>
    </div>
  )
}

function DetailPanel({
  hop,
  live,
  current,
  onFollow,
}: {
  hop: Hop
  live: boolean
  current: boolean
  onFollow: () => void
}) {
  const worker = workerById[hop.node]
  const rows: [string, string][] = [
    ['input', hop.input],
    ['output', hop.output],
    ['language', hop.lang],
    ['worker', `${worker.worker} (${worker.kind})`],
    ['location', hop.where],
    ['duration', `${hop.ms.toLocaleString('en-US')} ms`],
  ]
  return (
    <aside aria-live="polite" className="flex min-w-0 flex-col rounded-xl border bg-card lg:self-start">
      <div className="flex items-center justify-between gap-2 border-b px-4 py-3">
        <p className="min-w-0 truncate font-mono text-[13px] text-foreground">{hop.fn}</p>
        {live ? (
          <span className="flex shrink-0 items-center gap-1.5 font-sans text-[13px] text-muted-foreground">
            <span className={cn('size-1.5 rounded-full', current ? 'bg-ok' : 'bg-muted-foreground/50')} />
            live
          </span>
        ) : (
          <button
            type="button"
            onClick={onFollow}
            className="pressable shrink-0 rounded-md px-2 py-1 font-sans text-[13px] text-muted-foreground outline-none hover:bg-faint hover:text-foreground focus-visible:outline-2 focus-visible:outline-foreground"
          >
            follow execution
          </button>
        )}
      </div>
      <dl
        key={hop.fn}
        className="swap-in grid grid-cols-[84px_minmax(0,1fr)] gap-x-3 gap-y-2 px-4 py-3 font-sans text-[13px]"
      >
        {rows.map(([term, value]) => (
          <div key={term} className="contents">
            <dt className="text-muted-foreground">{term}</dt>
            <dd className="min-w-0 truncate font-mono text-foreground">{value}</dd>
          </div>
        ))}
      </dl>
      <div className="mt-auto border-t border-dashed px-4 py-3">
        <p className="font-sans text-[13px] text-muted-foreground uppercase tracking-[0.08em]">logs</p>
        <ul key={hop.fn} className="swap-in mt-2 flex flex-col gap-1 font-mono text-[13px] text-muted-foreground">
          {(demoLogs[hop.fn] ?? []).map((line) => (
            <li key={line} className="truncate">
              <span className="text-foreground/60">· </span>
              {line}
            </li>
          ))}
        </ul>
      </div>
      <p className="border-t px-4 py-2.5 font-sans text-[13px] text-muted-foreground">
        Click a node in the graph to inspect it.
      </p>
    </aside>
  )
}

function CodeView({ code, line, done }: { code: CodeLine[]; line: number | null; done: boolean }) {
  return (
    <div className="swap-in overflow-hidden rounded-xl border bg-card">
      <div className="flex h-11 items-center justify-between border-b px-4">
        <span className="font-mono text-muted-foreground text-xs">{demoCode.file}</span>
        <span className="font-sans text-[13px] text-muted-foreground">
          {done ? 'run complete' : line === null ? 'idle' : `executing line ${line + 1}`}
        </span>
      </div>
      <pre className="overflow-x-auto p-3.5 font-mono text-[13px] leading-[1.7] sm:p-4 sm:text-[13px] [scrollbar-width:thin]">
        <code className="code-tokens">
          {code.map((tokens, index) => (
            <span
              // biome-ignore lint/suspicious/noArrayIndexKey: Lines are static and never reorder.
              key={index}
              className={cn(
                'flex px-1 transition-colors duration-300',
                line === index ? 'bg-faint shadow-[inset_2px_0_0_0_var(--hero-accent)]' : '',
              )}
            >
              <span aria-hidden className="w-7 shrink-0 select-none text-muted-foreground/70">
                {line === index ? '▶' : index + 1}
              </span>
              <span className="whitespace-pre">
                {tokens.map((token, position) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: Tokens are static per line and never reorder.
                  <span key={position} style={token.style as CSSProperties}>
                    {token.text}
                  </span>
                ))}
                {'\n'}
              </span>
            </span>
          ))}
        </code>
      </pre>
    </div>
  )
}

/** Ruler marks on the real timeline (ms). */
const TICKS = [0, 1000, 2000, 3000]

function TraceView({ step }: { step: number }) {
  return (
    <div className="swap-in overflow-hidden rounded-xl border bg-card">
      <div className="flex h-11 items-center justify-between border-b px-4 font-sans text-xs">
        <span className="font-mono text-muted-foreground">trace 7f3a · agent::run</span>
        <span className="text-muted-foreground">{TOTAL_MS.toLocaleString('en-US')} ms</span>
      </div>
      <div className="grid grid-cols-[minmax(0,1fr)] gap-y-2 p-4 @container sm:grid-cols-[220px_minmax(0,1fr)] sm:gap-x-6">
        <div className="hidden sm:block" />
        <div className="relative hidden h-4 sm:block">
          {TICKS.map((tick) => (
            <span
              key={tick}
              className="absolute top-0 -translate-x-1/2 font-mono text-[10px] text-muted-foreground"
              style={{ left: `${(tick / TOTAL_MS) * 100}%` }}
            >
              {tick ? `${(tick / 1000).toFixed(1)}s` : '0'}
            </span>
          ))}
        </div>
        {HOPS.map((hop, i) => {
          const shown = step >= i
          const lit = step === i
          const left = (STARTS[i] / TOTAL_MS) * 100
          const width = Math.max(0.6, (hop.ms / TOTAL_MS) * 100)
          return (
            <div key={hop.fn} className="contents">
              <div
                className={cn(
                  'flex min-w-0 items-center gap-2.5 font-sans text-[13px] transition-colors duration-300',
                  i ? 'pl-5' : '',
                  shown ? 'text-foreground' : 'text-muted-foreground/60',
                )}
              >
                {i ? <span className="text-muted-foreground">└</span> : null}
                <span className="truncate font-mono">{hop.fn}</span>
                <span className="ml-auto shrink-0 text-muted-foreground tabular-nums">
                  {hop.ms.toLocaleString('en-US')}ms
                </span>
              </div>
              <div className="relative h-5">
                <span aria-hidden className="absolute inset-y-0 left-0 w-px bg-border" />
                <motion.span
                  aria-hidden
                  className={cn(
                    'absolute top-1/2 h-2.5 -translate-y-1/2 rounded-sm',
                    lit ? 'bg-hero-accent' : 'bg-foreground/60',
                  )}
                  style={{ left: `${left}%`, width: `${width}%`, transformOrigin: 'left center' }}
                  initial={false}
                  animate={{ scaleX: shown ? 1 : 0, opacity: shown ? 1 : 0 }}
                  transition={{ duration: 0.4, ease: easeOut }}
                />
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}
