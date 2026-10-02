'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'

import { cn } from '@/lib/utils'
import { useGraphicLoop } from './use-graphic-loop'

type P = { x: number; y: number }
type NodeId = 'py' | 'ts' | 'state' | 'http' | 'harness' | 'ade'
type Node = P & { id: NodeId; kind: string; label: string; hw: number; phase: 1 | 2 | 3 }

const C: P = { x: 260, y: 186 }
const ENGINE = { hw: 70, hh: 32 }
const HH = 17

const NODES: Node[] = [
  { id: 'py', kind: 'py', label: 'worker.py', x: 84, y: 186, hw: 62, phase: 1 },
  { id: 'ts', kind: 'ts', label: 'worker.ts', x: 436, y: 186, hw: 62, phase: 1 },
  { id: 'state', kind: 'kv', label: 'state', x: 150, y: 70, hw: 50, phase: 2 },
  { id: 'http', kind: 'http', label: 'POST /add', x: 370, y: 70, hw: 66, phase: 2 },
  { id: 'harness', kind: 'ai', label: 'harness', x: 150, y: 318, hw: 58, phase: 3 },
  { id: 'ade', kind: 'ui', label: 'ADE', x: 370, y: 318, hw: 58, phase: 3 },
]
const byId = Object.fromEntries(NODES.map((n) => [n.id, n])) as Record<NodeId, Node>

type Event = { phase: 1 | 2 | 3; from?: NodeId; to: NodeId; fn: string }

const SCRIPT: Event[] = [
  { phase: 1, from: 'ts', to: 'py', fn: 'math::add' },
  { phase: 1, from: 'py', to: 'ts', fn: 'format::sum' },
  { phase: 2, from: 'http', to: 'ts', fn: 'POST /add' },
  { phase: 2, from: 'py', to: 'state', fn: 'state::set' },
  { phase: 3, from: 'harness', to: 'py', fn: 'math::add' },
  { phase: 3, to: 'ade', fn: 'trace → ADE' },
]

const STEPS = [
  { n: 1, label: 'compose --up' },
  { n: 2, label: 'state + http' },
  { n: 3, label: 'harness + ADE' },
]

const STEP_MS = 2300
const ease = [0.22, 1, 0.36, 1] as const

function edge(center: P, toward: P, hw: number, hh: number): P {
  const dx = toward.x - center.x
  const dy = toward.y - center.y
  const t = Math.min(
    dx === 0 ? Number.POSITIVE_INFINITY : hw / Math.abs(dx),
    dy === 0 ? Number.POSITIVE_INFINITY : hh / Math.abs(dy),
  )
  return { x: center.x + dx * t, y: center.y + dy * t }
}

const nodeEdge = (n: Node) => edge(n, C, n.hw, HH)
const engineEdge = (n: Node) => edge(C, n, ENGINE.hw, ENGINE.hh)

/**
 * What the quickstart gives you, built up in three steps: a Python and a TypeScript worker
 * calling each other through the engine, then `state` and an HTTP route, then the optional
 * harness and ADE (dashed) on `localhost:3113`.
 */
export function GetStartedDiagram({ className }: { className?: string }) {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const [step, setStep] = useState(0)

  useEffect(() => {
    if (!active) return
    const id = window.setInterval(() => setStep((s) => s + 1), STEP_MS)
    return () => window.clearInterval(id)
  }, [active])

  const event = active ? SCRIPT[step % SCRIPT.length] : undefined
  const phase = event?.phase ?? 3
  const from = event?.from ? byId[event.from] : undefined
  const to = event ? byId[event.to] : undefined

  return (
    <div ref={ref} className={cn('relative', className)}>
      <svg
        viewBox="0 0 520 380"
        role="img"
        aria-label="The quickstart runs the engine with a Python worker and a TypeScript worker that call each other through it, plus a state worker and an HTTP route. The optional step adds the harness and the ADE at localhost:3113."
        className="h-auto w-full overflow-visible"
      >
        {/* Optional zone */}
        <motion.g initial={false} animate={{ opacity: phase >= 3 ? 1 : 0.35 }} transition={{ duration: 0.6, ease }}>
          <rect
            x={40}
            y={270}
            width={440}
            height={100}
            rx={14}
            fill="none"
            stroke="var(--line-strong)"
            strokeDasharray="3 4"
          />
          <Badge x={60} y={290} n={3} on={phase === 3 && active} />
          <text x={74} y={293.5} className="fill-muted-foreground font-mono" fontSize={10} letterSpacing="0.08em">
            OPTIONAL
          </text>
          <text x={370} y={354} textAnchor="middle" className="fill-muted-foreground font-mono" fontSize={10.5}>
            localhost:3113
          </text>
        </motion.g>

        {/* Links */}
        {NODES.map((n) => {
          const a = engineEdge(n)
          const b = nodeEdge(n)
          const on = n.phase <= phase
          return (
            <motion.line
              key={n.id}
              x1={a.x}
              y1={a.y}
              x2={b.x}
              y2={b.y}
              stroke="var(--line-strong)"
              strokeDasharray={n.phase === 3 ? '3 4' : undefined}
              initial={false}
              animate={{ pathLength: on ? 1 : 0, opacity: on ? 1 : 0 }}
              transition={{ duration: 0.7, ease }}
            />
          )
        })}

        {/* Route */}
        <AnimatePresence>
          {event && to ? (
            <motion.g key={`route-${step}`} exit={{ opacity: 0 }} transition={{ duration: 0.4 }}>
              {from ? <Seg a={nodeEdge(from)} b={engineEdge(from)} delay={0.35} /> : null}
              <Seg a={engineEdge(to)} b={nodeEdge(to)} delay={from ? 1.0 : 0.35} />
            </motion.g>
          ) : null}
        </AnimatePresence>

        {/* Engine */}
        <rect
          x={C.x - ENGINE.hw}
          y={C.y - ENGINE.hh}
          width={ENGINE.hw * 2}
          height={ENGINE.hh * 2}
          rx={16}
          fill="var(--node)"
          stroke="var(--line-strong)"
        />
        <g transform={`translate(${C.x - 34} ${C.y - 16.5}) scale(0.0145)`} fill="var(--foreground)">
          <rect width="233.4" height="233.4" />
          <rect y="350.1" width="233.4" height="700.21" />
          <rect x="350.1" width="233.4" height="233.4" />
          <rect x="350.1" y="350.1" width="233.4" height="700.21" />
          <rect x="700.21" width="233.4" height="233.4" />
          <rect x="700.21" y="350.1" width="233.4" height="700.21" />
        </g>
        <text
          x={C.x - 12}
          y={C.y - 5.5}
          className="fill-muted-foreground font-mono"
          fontSize={10}
          letterSpacing="0.12em"
        >
          ENGINE
        </text>
        <AnimatePresence mode="wait" initial={false}>
          <motion.text
            key={event ? step : 'static'}
            x={C.x}
            y={C.y + 16}
            textAnchor="middle"
            className="fill-foreground font-mono"
            fontSize={11}
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={{ duration: 0.3, ease }}
          >
            {event?.fn ?? 'math::add'}
          </motion.text>
        </AnimatePresence>

        {/* Step badges */}
        <Badge x={30} y={150} n={1} on={phase === 1 && active} />
        <motion.g initial={false} animate={{ opacity: phase >= 2 ? 1 : 0.35 }} transition={{ duration: 0.6 }}>
          <Badge x={80} y={70} n={2} on={phase === 2 && active} />
        </motion.g>

        {/* Nodes */}
        {NODES.map((n) => {
          const on = n.phase <= phase
          const hot = event && (event.from === n.id || event.to === n.id)
          const chipW = Math.max(18, n.kind.length * 5.4 + 8)
          return (
            <motion.g
              key={n.id}
              initial={false}
              animate={{ opacity: on ? 1 : 0.18, scale: on ? 1 : 0.96 }}
              transition={{ duration: 0.6, ease }}
              style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
            >
              <rect
                x={n.x - n.hw}
                y={n.y - HH}
                width={n.hw * 2}
                height={HH * 2}
                rx={10}
                fill="var(--node)"
                stroke={hot ? 'var(--hero-accent)' : 'var(--line-strong)'}
                strokeDasharray={n.phase === 3 ? '3 3' : undefined}
                style={{ transition: 'stroke 300ms ease' }}
              />
              <rect
                x={n.x - n.hw + 8}
                y={n.y - 9}
                width={chipW}
                height={18}
                rx={5}
                fill="var(--faint)"
                stroke="var(--line)"
              />
              <text
                x={n.x - n.hw + 8 + chipW / 2}
                y={n.y + 3.2}
                textAnchor="middle"
                className="fill-muted-foreground font-mono"
                fontSize={8.5}
              >
                {n.kind}
              </text>
              <text x={n.x - n.hw + chipW + 16} y={n.y + 4} className="fill-foreground font-mono" fontSize={11}>
                {n.label}
              </text>
              {event && event.to === n.id ? (
                <motion.circle
                  cx={n.x + n.hw - 10}
                  cy={n.y}
                  r={2.5}
                  fill="var(--ok)"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: [0, 1, 1, 0] }}
                  transition={{ duration: 1, delay: from ? 1.6 : 0.95, times: [0, 0.1, 0.7, 1] }}
                />
              ) : null}
            </motion.g>
          )
        })}

        {/* Packets */}
        <AnimatePresence>
          {event && to ? (
            <motion.g key={`packet-${step}`} exit={{ opacity: 0 }}>
              {from ? <Packet a={nodeEdge(from)} b={engineEdge(from)} delay={0.35} /> : null}
              <Packet a={engineEdge(to)} b={nodeEdge(to)} delay={from ? 1.0 : 0.35} />
            </motion.g>
          ) : null}
        </AnimatePresence>
      </svg>

      <ol className="mt-4 flex flex-wrap items-center justify-center gap-x-5 gap-y-2 font-mono text-[12px]">
        {STEPS.map((s) => (
          <li
            key={s.n}
            className={cn(
              'flex items-center gap-2 transition-colors duration-300',
              !active || phase === s.n ? 'text-foreground' : 'text-muted-foreground',
            )}
          >
            <span
              className={cn(
                'flex size-[18px] items-center justify-center rounded-full border text-[11px] transition-colors duration-300',
                active && phase === s.n ? 'border-foreground bg-foreground text-background' : 'bg-background',
                s.n === 3 && 'border-dashed',
              )}
            >
              {s.n}
            </span>
            {s.label}
            {s.n === 3 ? <span className="text-muted-foreground">(optional)</span> : null}
          </li>
        ))}
      </ol>
    </div>
  )
}

function Badge({ x, y, n, on }: { x: number; y: number; n: number; on: boolean }) {
  return (
    <g>
      <circle
        cx={x}
        cy={y}
        r={9}
        fill={on ? 'var(--foreground)' : 'var(--node)'}
        stroke={on ? 'var(--hero-accent)' : 'var(--line-strong)'}
        style={{ transition: 'fill 300ms ease, stroke 300ms ease' }}
      />
      <text
        x={x}
        y={y + 3.5}
        textAnchor="middle"
        fontSize={10}
        className={cn('font-mono', on ? 'fill-background' : 'fill-muted-foreground')}
      >
        {n}
      </text>
    </g>
  )
}

function Seg({ a, b, delay }: { a: P; b: P; delay: number }) {
  return (
    <motion.line
      x1={a.x}
      y1={a.y}
      x2={b.x}
      y2={b.y}
      stroke="var(--hero-accent)"
      strokeWidth={1.25}
      initial={{ pathLength: 0, opacity: 0.9 }}
      animate={{ pathLength: 1, opacity: [0.9, 0.9, 0.25] }}
      transition={{
        pathLength: { duration: 0.6, delay, ease: 'easeInOut' },
        opacity: { duration: 1.6, delay, times: [0, 0.6, 1] },
      }}
    />
  )
}

function Packet({ a, b, delay }: { a: P; b: P; delay: number }) {
  return (
    <motion.circle
      r={3.5}
      fill="var(--foreground)"
      initial={{ cx: a.x, cy: a.y, opacity: 0 }}
      animate={{ cx: b.x, cy: b.y, opacity: [0, 1, 1, 0] }}
      transition={{
        cx: { duration: 0.6, delay, ease: 'easeInOut' },
        cy: { duration: 0.6, delay, ease: 'easeInOut' },
        opacity: { duration: 0.6, delay, times: [0, 0.15, 0.85, 1] },
      }}
    />
  )
}
