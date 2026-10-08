'use client'

import { AnimatePresence, motion } from 'motion/react'
import { createContext, useContext, useEffect, useMemo, useState } from 'react'

import { cn } from '@/lib/utils'
import { useGraphicLoop } from './use-graphic-loop'

type Column = 'worker' | 'function' | 'trigger'

const FocusContext = createContext<{ focus: Column | null; setFocus: (c: Column | null) => void } | null>(null)

function useFocus() {
  const ctx = useContext(FocusContext)
  if (!ctx) throw new Error('ModelFocus is missing')
  return ctx
}

/** Lets a hovered primitive card highlight its column in the registry. */
export function ModelFocus({ children }: { children: React.ReactNode }) {
  const [focus, setFocus] = useState<Column | null>(null)
  const value = useMemo(() => ({ focus, setFocus }), [focus])
  return <FocusContext.Provider value={value}>{children}</FocusContext.Provider>
}

/** A primitive card that reports hover and focus to the registry graphic. */
export function ModelCard({
  column,
  className,
  children,
}: {
  column: Column
  className?: string
  children: React.ReactNode
}) {
  const { focus, setFocus } = useFocus()
  return (
    <article
      onPointerEnter={() => setFocus(column)}
      onPointerLeave={() => setFocus(null)}
      onFocus={() => setFocus(column)}
      onBlur={() => setFocus(null)}
      data-focused={focus === column || undefined}
      className={cn(
        'group/card flex flex-col overflow-hidden rounded-xl border bg-card transition-colors duration-300 data-[focused]:border-line-strong',
        className,
      )}
    >
      {children}
    </article>
  )
}

const ease = [0.22, 1, 0.36, 1] as const

type WorkerId = 'agent' | 'billing' | 'api' | 'queue' | 'ci'
type Row = { id: WorkerId; worker: string; kind: string; fn: string; trigger: string; detail: string }

const ROWS: Row[] = [
  { id: 'agent', worker: 'agent', kind: 'ai', fn: 'agent::triage', trigger: 'queue', detail: 'support' },
  { id: 'billing', worker: 'billing.py', kind: 'py', fn: 'billing::charge', trigger: 'cron', detail: '0 0 9 * * * *' },
  { id: 'api', worker: 'orders-api', kind: 'ts', fn: 'orders::refund', trigger: 'http', detail: 'POST /refunds' },
  { id: 'queue', worker: 'queue', kind: 'q', fn: 'queue::enqueue', trigger: 'call', detail: 'direct' },
  { id: 'ci', worker: 'ci-runner', kind: 'ci', fn: 'ci::test', trigger: 'event', detail: 'git.push' },
]

type Step =
  | { type: 'call'; to: WorkerId; fn: string; caption: string }
  | { type: 'join' | 'leave'; caption: string }
  | { type: 'blocked'; caption: string }

const SCRIPT: Step[] = [
  { type: 'call', to: 'api', fn: 'orders::refund', caption: 'agent → engine → orders::refund → orders-api' },
  { type: 'join', caption: 'ci-runner connected · ci::test registered' },
  { type: 'call', to: 'ci', fn: 'ci::test', caption: 'agent → engine → ci::test → ci-runner' },
  { type: 'leave', caption: 'ci-runner dropped · ci::test removed from the registry' },
  { type: 'blocked', caption: 'agent → orders-api directly · there is no such path' },
]

const STEP_MS = 2600

/** ci-runner is registered during these steps. */
const CI_PRESENT = new Set([1, 2])

type StripNode = { id: WorkerId; label: string; kind: string; x: number }

const HW = 50
const HH = 17
const NODE_Y = 74

function StripSvg({
  nodes,
  width,
  step,
  ciPresent,
  live,
  className,
}: {
  nodes: StripNode[]
  width: number
  step: number
  ciPresent: boolean
  live: boolean
  className?: string
}) {
  const s = SCRIPT[step % SCRIPT.length]
  const byId = Object.fromEntries(nodes.map((n) => [n.id, n])) as Partial<Record<WorkerId, StripNode>>
  const agent = byId.agent as StripNode
  const api = byId.api as StripNode
  const bottom = 118
  const top = NODE_Y - HH
  // Crossed-out direct path, arcing over the row.
  const mid = (agent.x + api.x) / 2
  const apexCtrl = top - 60
  const apexY = (top + 2 * apexCtrl + top) / 4
  const target = s.type === 'call' ? byId[s.to] : undefined

  return (
    <svg
      viewBox={`0 0 ${width} 120`}
      className={cn('block h-auto w-full overflow-visible', className)}
      role="img"
      aria-label="Workers connect only to the engine below them. The direct path between two workers is crossed out."
    >
      <path
        d={`M${agent.x} ${top} Q${mid} ${apexCtrl} ${api.x} ${top}`}
        fill="none"
        stroke="var(--line-strong)"
        strokeDasharray="3 4"
      />
      <g
        stroke={live && s.type === 'blocked' ? 'var(--fail)' : 'var(--muted-foreground)'}
        strokeWidth={1.5}
        strokeLinecap="round"
        style={{ transition: 'stroke 250ms ease 500ms' }}
      >
        <line x1={mid - 4.5} y1={apexY - 4.5} x2={mid + 4.5} y2={apexY + 4.5} />
        <line x1={mid - 4.5} y1={apexY + 4.5} x2={mid + 4.5} y2={apexY - 4.5} />
      </g>
      <rect x={mid - 46} y={apexY + 9} width={92} height={13} fill="var(--card)" />
      <text x={mid} y={apexY + 19} textAnchor="middle" fontSize={9.5} className="fill-muted-foreground font-mono">
        no direct calls
      </text>

      {nodes.map((n) => {
        const visible = n.id !== 'ci' || ciPresent
        const lit = live && s.type !== 'join' && s.type !== 'leave' && (n.id === 'agent' || n.id === target?.id)
        return (
          <motion.g
            key={n.id}
            initial={false}
            animate={{ opacity: visible ? 1 : 0, y: visible ? 0 : -6 }}
            transition={{ duration: 0.6, ease }}
          >
            <motion.line
              x1={n.x}
              y1={NODE_Y + HH}
              x2={n.x}
              y2={bottom + 2}
              stroke="var(--line-strong)"
              strokeDasharray={n.id === 'ci' ? '3 4' : undefined}
              initial={false}
              animate={{ pathLength: visible ? 1 : 0 }}
              transition={{ duration: 0.6, ease }}
            />
            <rect
              x={n.x - HW}
              y={NODE_Y - HH}
              width={HW * 2}
              height={HH * 2}
              rx={10}
              fill="var(--node)"
              stroke={lit ? 'var(--foreground)' : 'var(--line-strong)'}
              strokeDasharray={n.id === 'ci' ? '3 3' : undefined}
              style={{ transition: 'stroke 300ms ease' }}
            />
            <rect
              x={n.x - HW + 7}
              y={NODE_Y - 9}
              width={18}
              height={18}
              rx={5}
              fill="var(--faint)"
              stroke="var(--line)"
            />
            <text
              x={n.x - HW + 16}
              y={NODE_Y + 3.2}
              textAnchor="middle"
              fontSize={8.5}
              className="fill-muted-foreground font-mono"
            >
              {n.kind}
            </text>
            <text x={n.x - HW + 31} y={NODE_Y + 4} fontSize={10.5} className="fill-foreground font-mono">
              {n.label}
            </text>
          </motion.g>
        )
      })}

      <AnimatePresence>
        {live && s.type === 'call' && target ? (
          <motion.g key={`call-${step}`} exit={{ opacity: 0 }} transition={{ duration: 0.3 }}>
            <Packet x={agent.x} from={NODE_Y + HH} to={bottom} delay={0} />
            <Packet x={target.x} from={bottom} to={NODE_Y + HH} delay={1.1} />
          </motion.g>
        ) : null}
        {live && s.type === 'blocked' ? (
          <motion.circle
            key={`blocked-${step}`}
            r={3.25}
            fill="var(--foreground)"
            initial={{ cx: agent.x, cy: top, opacity: 0 }}
            animate={{
              cx: [agent.x, (agent.x + mid) / 2 - 8, mid - 10],
              cy: [top, apexY + 6, apexY + 1],
              opacity: [0, 1, 0],
            }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.6, ease: 'easeOut' }}
          />
        ) : null}
      </AnimatePresence>
    </svg>
  )
}

function Packet({ x, from, to, delay }: { x: number; from: number; to: number; delay: number }) {
  return (
    <motion.circle
      r={3.25}
      cx={x}
      fill="var(--foreground)"
      initial={{ cy: from, opacity: 0 }}
      animate={{ cy: to, opacity: [0, 1, 1, 0] }}
      transition={{ duration: 0.5, delay, ease: 'easeInOut' }}
    />
  )
}

const DESKTOP: StripNode[] = [
  { id: 'agent', label: 'agent', kind: 'ai', x: 60 },
  { id: 'billing', label: 'billing.py', kind: 'py', x: 170 },
  { id: 'api', label: 'orders-api', kind: 'ts', x: 280 },
  { id: 'queue', label: 'queue', kind: 'q', x: 390 },
  { id: 'ci', label: 'ci-runner', kind: 'ci', x: 500 },
]
const MOBILE: StripNode[] = [
  { id: 'agent', label: 'agent', kind: 'ai', x: 58 },
  { id: 'api', label: 'orders-api', kind: 'ts', x: 170 },
  { id: 'ci', label: 'ci-runner', kind: 'ci', x: 282 },
]

const COLS = 'grid-cols-[minmax(0,0.95fr)_minmax(0,1.05fr)_minmax(0,1.1fr)]'

/**
 * Model graphic: the engine's live registry of workers, functions, and triggers. Rows appear
 * and disappear as workers connect and drop, every call is routed through the engine, and
 * the direct path between two workers does not exist.
 */
export function ModelRegistry({ className }: { className?: string }) {
  const { ref, active, reduce } = useGraphicLoop<HTMLDivElement>()
  const { focus } = useFocus()
  const [step, setStep] = useState(0)

  useEffect(() => {
    if (!active) return
    const id = window.setInterval(() => setStep((s) => s + 1), STEP_MS)
    return () => window.clearInterval(id)
  }, [active])

  const i = step % SCRIPT.length
  const s = SCRIPT[i]
  const ciPresent = reduce || CI_PRESENT.has(i)
  const leaving = active && s.type === 'leave'
  const rows = ROWS.filter((r) => r.id !== 'ci' || ciPresent || leaving)
  const routed = active && s.type === 'call' ? s.fn : null
  const joined = active && s.type === 'join'
  const dim = (c: Column) => (focus && focus !== c ? 'opacity-35' : 'opacity-100')

  return (
    <div ref={ref} className={cn('overflow-hidden rounded-xl border bg-card', className)}>
      <div className="relative px-3 pt-4 sm:px-5">
        <div
          aria-hidden
          className="bg-dots pointer-events-none absolute inset-0 [mask-image:linear-gradient(to_bottom,black,transparent)]"
        />
        <StripSvg
          nodes={DESKTOP}
          width={560}
          step={step}
          ciPresent={ciPresent}
          live={active}
          className="relative hidden sm:block"
        />
        <StripSvg
          nodes={MOBILE}
          width={340}
          step={step}
          ciPresent={ciPresent}
          live={active}
          className="relative sm:hidden"
        />
      </div>

      <div className="relative mx-3 mb-3 rounded-lg border border-line-strong bg-background/60 sm:mx-5 sm:mb-5">
        <div className="flex items-center justify-between gap-3 border-b px-3 py-2.5 sm:px-4">
          <div className="flex items-center gap-2 font-mono text-[12px] text-muted-foreground uppercase tracking-[0.08em]">
            <svg viewBox="0 0 933.61 1050.31" aria-hidden="true" className="h-3 w-auto fill-foreground">
              <rect width="233.4" height="233.4" />
              <rect y="350.1" width="233.4" height="700.21" />
              <rect x="350.1" width="233.4" height="233.4" />
              <rect x="350.1" y="350.1" width="233.4" height="700.21" />
              <rect x="700.21" width="233.4" height="233.4" />
              <rect x="700.21" y="350.1" width="233.4" height="700.21" />
            </svg>
            Engine · live registry
          </div>
          <div className="flex items-center gap-2 font-mono text-[12px] text-muted-foreground tabular-nums">
            <span className="relative flex size-2 items-center justify-center">
              <span className="absolute size-2 animate-ping rounded-full bg-ok/40 motion-reduce:animate-none" />
              <span className="size-1.5 rounded-full bg-ok" />
            </span>
            {rows.length - (leaving ? 1 : 0)} workers
          </div>
        </div>

        <div className="px-1.5 py-1.5 font-mono text-[12px] sm:px-2 sm:text-xs">
          <div className={cn('grid gap-3 px-2 py-1.5 text-muted-foreground sm:px-2.5', COLS)}>
            {(['worker', 'function', 'trigger'] as const).map((c) => (
              <span
                key={c}
                className={cn(
                  'uppercase tracking-[0.08em] transition-[opacity,color] duration-300',
                  dim(c),
                  focus === c && 'text-foreground',
                )}
              >
                {c}
              </span>
            ))}
          </div>
          <AnimatePresence initial={false}>
            {rows.map((r) => {
              const isLeaving = leaving && r.id === 'ci'
              const isRouted = routed === r.fn
              const isJoined = joined && r.id === 'ci'
              return (
                <motion.div
                  key={r.id}
                  initial={{ opacity: 0, height: 0 }}
                  animate={{ opacity: isLeaving ? 0.45 : 1, height: 'auto' }}
                  exit={{ opacity: 0, height: 0 }}
                  transition={{ duration: 0.5, ease }}
                  className="overflow-hidden"
                >
                  <div
                    className={cn(
                      'relative grid items-center gap-3 rounded-md border px-2 py-2 transition-colors duration-300 sm:px-2.5',
                      COLS,
                      isRouted || isJoined ? 'border-line-strong bg-faint' : 'border-transparent',
                    )}
                  >
                    <span
                      className={cn('flex min-w-0 items-center gap-2 transition-opacity duration-300', dim('worker'))}
                    >
                      <span
                        className={cn(
                          'size-1.5 shrink-0 rounded-full transition-colors duration-300',
                          isLeaving ? 'bg-fail' : 'bg-ok',
                        )}
                      />
                      <span
                        className={cn(
                          'truncate text-foreground',
                          isLeaving && 'line-through decoration-muted-foreground',
                        )}
                      >
                        {r.worker}
                      </span>
                    </span>
                    <span
                      className={cn(
                        'truncate transition-[opacity,color] duration-300',
                        dim('function'),
                        isRouted ? 'text-foreground' : 'text-muted-foreground',
                        focus === 'function' && 'text-foreground',
                      )}
                    >
                      {r.fn}
                    </span>
                    <span
                      className={cn(
                        'flex min-w-0 items-center gap-1.5 transition-opacity duration-300',
                        dim('trigger'),
                      )}
                    >
                      <span
                        className={cn(
                          'shrink-0 rounded-[5px] border bg-card px-1.5 py-px text-[11px] transition-colors duration-300',
                          focus === 'trigger' ? 'border-line-strong text-foreground' : 'text-muted-foreground',
                        )}
                      >
                        {r.trigger}
                      </span>
                      <span className="truncate text-muted-foreground">{r.detail}</span>
                    </span>
                  </div>
                </motion.div>
              )
            })}
          </AnimatePresence>
        </div>
      </div>

      <div className="flex h-10 items-center justify-center overflow-hidden border-t px-4 text-center font-mono text-muted-foreground text-xs">
        <AnimatePresence mode="wait" initial={false}>
          <motion.span
            key={active ? i : 'static'}
            className="truncate"
            initial={{ opacity: 0, y: 6, filter: 'blur(2px)' }}
            animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
            exit={{ opacity: 0, y: -6, filter: 'blur(2px)' }}
            transition={{ duration: 0.3, ease }}
          >
            {active ? s.caption : 'every call is routed through the engine'}
          </motion.span>
        </AnimatePresence>
      </div>
    </div>
  )
}
