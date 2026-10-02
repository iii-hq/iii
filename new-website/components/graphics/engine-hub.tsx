'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'

import { cn } from '@/lib/utils'
import { useGraphicLoop } from './use-graphic-loop'

type Point = { x: number; y: number }
type WorkerNode = Point & { id: string; label: string; kind: string; ephemeral?: boolean }

const C: Point = { x: 280, y: 260 }
const ENGINE = { hw: 78, hh: 48 }
const NODE = { hw: 58, hh: 18 }

function slot(deg: number): Point {
  const r = (deg * Math.PI) / 180
  return { x: C.x + 212 * Math.cos(r), y: C.y + 196 * Math.sin(r) }
}

const WORKERS: WorkerNode[] = [
  { id: 'agent', label: 'claude-code', kind: 'ai', ...slot(-90) },
  { id: 'billing', label: 'billing.py', kind: 'py', ...slot(-45) },
  { id: 'resize', label: 'resize.rs', kind: 'rs', ...slot(0) },
  { id: 'pi', label: 'raspberry-pi', kind: 'pi', ...slot(45) },
  { id: 'ci', label: 'ci-runner', kind: 'ci', ephemeral: true, ...slot(90) },
  { id: 'browser', label: 'browser tab', kind: 'js', ...slot(135) },
  { id: 'api', label: 'orders-api', kind: 'ts', ...slot(180) },
  { id: 'queue', label: 'queue', kind: 'q', ...slot(-135) },
]

const byId = Object.fromEntries(WORKERS.map((w) => [w.id, w])) as Record<string, WorkerNode>

type Event =
  | { type: 'call'; from: string; to: string; fn: string }
  | { type: 'join' | 'leave'; node: string; fn: string }

const SCRIPT: Event[] = [
  { type: 'call', from: 'agent', to: 'api', fn: 'orders::refund' },
  { type: 'call', from: 'api', to: 'billing', fn: 'billing::charge' },
  { type: 'call', from: 'queue', to: 'resize', fn: 'images::resize' },
  { type: 'join', node: 'ci', fn: 'ci::test' },
  { type: 'call', from: 'agent', to: 'ci', fn: 'ci::test' },
  { type: 'call', from: 'browser', to: 'pi', fn: 'sensors::read' },
  { type: 'leave', node: 'ci', fn: 'ci::test' },
  { type: 'call', from: 'pi', to: 'queue', fn: 'queue::enqueue' },
]

/** Where the segment from `center` toward `toward` exits a box of half-size hw×hh. */
function edge(center: Point, toward: Point, hw: number, hh: number): Point {
  const dx = toward.x - center.x
  const dy = toward.y - center.y
  const t = Math.min(
    dx === 0 ? Number.POSITIVE_INFINITY : hw / Math.abs(dx),
    dy === 0 ? Number.POSITIVE_INFINITY : hh / Math.abs(dy),
  )
  return { x: center.x + dx * t, y: center.y + dy * t }
}

function ciPresentAt(step: number) {
  let present = false
  for (let i = 0; i <= step; i++) {
    const e = SCRIPT[i % SCRIPT.length]
    if (e.type === 'join') present = true
    if (e.type === 'leave') present = false
  }
  return present
}

const STEP_MS = 2400
const ease = [0.22, 1, 0.36, 1] as const

/**
 * Hero graphic: workers of every kind hold one WebSocket to the engine. Calls go
 * caller → engine → owner, never worker-to-worker, and a CI runner joins and leaves.
 */
export function EngineHub({ className }: { className?: string }) {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const [step, setStep] = useState(0)

  useEffect(() => {
    if (!active) return
    const id = window.setInterval(() => setStep((s) => s + 1), STEP_MS)
    return () => window.clearInterval(id)
  }, [active])

  const event = SCRIPT[step % SCRIPT.length]
  const ciPresent = ciPresentAt(step)
  const from = event.type === 'call' ? byId[event.from] : byId[event.node]
  const to = event.type === 'call' ? byId[event.to] : undefined

  const caption =
    event.type === 'call'
      ? `${byId[event.from].label} → ${event.fn} → ${byId[event.to].label}`
      : event.type === 'join'
        ? `${byId[event.node].label} joined · registered ${event.fn}`
        : `${byId[event.node].label} left · ${event.fn} removed`

  return (
    <div ref={ref} className={cn('relative', className)}>
      <svg
        viewBox="0 0 560 520"
        role="img"
        aria-label="Workers written in different languages and running on different machines each hold one WebSocket connection to the iii engine, which routes every call between them."
        className="h-auto w-full overflow-visible"
      >
        <defs>
          <radialGradient id="hub-glow" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stopColor="var(--foreground)" stopOpacity="0.09" />
            <stop offset="100%" stopColor="var(--foreground)" stopOpacity="0" />
          </radialGradient>
        </defs>

        <ellipse cx={C.x} cy={C.y} rx={170} ry={150} fill="url(#hub-glow)" />
        <ellipse
          cx={C.x}
          cy={C.y}
          rx={212}
          ry={196}
          fill="none"
          stroke="var(--line)"
          strokeDasharray="2 6"
          strokeLinecap="round"
        />

        {/* WebSocket links */}
        {WORKERS.map((w) => {
          const a = edge(C, w, ENGINE.hw, ENGINE.hh)
          const b = edge(w, C, NODE.hw, NODE.hh)
          const visible = !w.ephemeral || ciPresent
          return (
            <motion.line
              key={w.id}
              x1={a.x}
              y1={a.y}
              x2={b.x}
              y2={b.y}
              stroke="var(--line-strong)"
              strokeWidth={1}
              strokeDasharray={w.ephemeral ? '3 4' : undefined}
              initial={false}
              animate={{ pathLength: visible ? 1 : 0, opacity: visible ? 1 : 0 }}
              transition={{ duration: 0.8, ease }}
            />
          )
        })}

        {/* Active route highlight */}
        <AnimatePresence>
          {active && event.type === 'call' && to ? (
            <motion.g key={`route-${step}`} exit={{ opacity: 0 }} transition={{ duration: 0.4 }}>
              <RouteSegment from={edge(from, C, NODE.hw, NODE.hh)} to={edge(C, from, ENGINE.hw, ENGINE.hh)} delay={0} />
              <RouteSegment from={edge(C, to, ENGINE.hw, ENGINE.hh)} to={edge(to, C, NODE.hw, NODE.hh)} delay={0.75} />
            </motion.g>
          ) : null}
        </AnimatePresence>

        {/* Engine */}
        <g>
          <AnimatePresence>
            {active && event.type === 'call' ? (
              <motion.rect
                key={`pulse-${step}`}
                x={C.x - ENGINE.hw}
                y={C.y - ENGINE.hh}
                width={ENGINE.hw * 2}
                height={ENGINE.hh * 2}
                rx={18}
                fill="none"
                stroke="var(--foreground)"
                style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
                initial={{ opacity: 0.5, scale: 1 }}
                animate={{ opacity: 0, scale: 1.18 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.9, delay: 0.6, ease: 'easeOut' }}
              />
            ) : null}
          </AnimatePresence>
          <rect
            x={C.x - ENGINE.hw}
            y={C.y - ENGINE.hh}
            width={ENGINE.hw * 2}
            height={ENGINE.hh * 2}
            rx={18}
            fill="var(--node)"
            stroke="var(--line-strong)"
          />
          <g transform={`translate(${C.x - 13} ${C.y - 30}) scale(0.028)`} fill="var(--foreground)">
            <rect width="233.4" height="233.4" />
            <rect y="350.1" width="233.4" height="700.21" />
            <rect x="350.1" width="233.4" height="233.4" />
            <rect x="350.1" y="350.1" width="233.4" height="700.21" />
            <rect x="700.21" width="233.4" height="233.4" />
            <rect x="700.21" y="350.1" width="233.4" height="700.21" />
          </g>
          <text
            x={C.x}
            y={C.y + 14}
            textAnchor="middle"
            className="fill-muted-foreground font-mono"
            fontSize={10}
            letterSpacing="0.12em"
          >
            ENGINE
          </text>
          <AnimatePresence mode="wait" initial={false}>
            <motion.text
              key={event.fn}
              x={C.x}
              y={C.y + 32}
              textAnchor="middle"
              className="fill-foreground font-mono"
              fontSize={11}
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -4 }}
              transition={{ duration: 0.3, ease }}
            >
              {event.fn}
            </motion.text>
          </AnimatePresence>
        </g>

        {/* Workers */}
        {WORKERS.map((w) => {
          const visible = !w.ephemeral || ciPresent
          const isTarget = active && event.type === 'call' && event.to === w.id
          const isSource = active && event.type === 'call' && event.from === w.id
          return (
            <motion.g
              key={w.id}
              initial={false}
              animate={{ opacity: visible ? 1 : 0, scale: visible ? 1 : 0.9 }}
              transition={{ duration: 0.6, ease }}
              style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
            >
              <rect
                x={w.x - NODE.hw}
                y={w.y - NODE.hh}
                width={NODE.hw * 2}
                height={NODE.hh * 2}
                rx={10}
                fill="var(--node)"
                stroke={isSource || isTarget ? 'var(--foreground)' : 'var(--line-strong)'}
                strokeDasharray={w.ephemeral ? '3 3' : undefined}
                style={{ transition: 'stroke 300ms ease' }}
              />
              <rect
                x={w.x - NODE.hw + 8}
                y={w.y - 9}
                width={18}
                height={18}
                rx={5}
                fill="var(--faint)"
                stroke="var(--line)"
              />
              <text
                x={w.x - NODE.hw + 17}
                y={w.y + 3.2}
                textAnchor="middle"
                className="fill-muted-foreground font-mono"
                fontSize={8.5}
              >
                {w.kind}
              </text>
              <text x={w.x - NODE.hw + 33} y={w.y + 4} className="fill-foreground font-mono" fontSize={11}>
                {w.label}
              </text>
              {isTarget ? (
                <motion.circle
                  cx={w.x + NODE.hw - 10}
                  cy={w.y}
                  r={2.5}
                  fill="var(--ok)"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: [0, 1, 1, 0] }}
                  transition={{ duration: 1.2, delay: 1.3, times: [0, 0.1, 0.7, 1] }}
                />
              ) : null}
            </motion.g>
          )
        })}

        {/* Packets */}
        <AnimatePresence>
          {active && event.type === 'call' && to ? (
            <motion.g key={`packet-${step}`} exit={{ opacity: 0 }}>
              <Packet from={edge(from, C, NODE.hw, NODE.hh)} to={edge(C, from, ENGINE.hw, ENGINE.hh)} delay={0} />
              <Packet from={edge(C, to, ENGINE.hw, ENGINE.hh)} to={edge(to, C, NODE.hw, NODE.hh)} delay={0.75} />
            </motion.g>
          ) : null}
        </AnimatePresence>
      </svg>

      <div className="mt-2 flex h-6 items-center justify-center overflow-hidden font-mono text-muted-foreground text-xs">
        <AnimatePresence mode="wait" initial={false}>
          <motion.span
            key={step % SCRIPT.length}
            initial={{ opacity: 0, y: 6, filter: 'blur(2px)' }}
            animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
            exit={{ opacity: 0, y: -6, filter: 'blur(2px)' }}
            transition={{ duration: 0.3, ease }}
          >
            {caption}
          </motion.span>
        </AnimatePresence>
      </div>
    </div>
  )
}

function RouteSegment({ from, to, delay }: { from: Point; to: Point; delay: number }) {
  return (
    <motion.line
      x1={from.x}
      y1={from.y}
      x2={to.x}
      y2={to.y}
      stroke="var(--foreground)"
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

function Packet({ from, to, delay }: { from: Point; to: Point; delay: number }) {
  return (
    <motion.circle
      r={3.5}
      fill="var(--foreground)"
      initial={{ cx: from.x, cy: from.y, opacity: 0 }}
      animate={{ cx: to.x, cy: to.y, opacity: [0, 1, 1, 0] }}
      transition={{
        cx: { duration: 0.6, delay, ease: 'easeInOut' },
        cy: { duration: 0.6, delay, ease: 'easeInOut' },
        opacity: { duration: 0.6, delay, times: [0, 0.15, 0.85, 1] },
      }}
    />
  )
}
