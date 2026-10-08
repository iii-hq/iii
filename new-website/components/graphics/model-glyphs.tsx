'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'

import { cn } from '@/lib/utils'
import { useGraphicLoop } from './use-graphic-loop'

const ease = [0.22, 1, 0.36, 1] as const

/** Increments while the graphic is on screen and motion is allowed. */
function useCycle(ms: number, active: boolean) {
  const [n, setN] = useState(0)
  useEffect(() => {
    if (!active) return
    const id = window.setInterval(() => setN((c) => c + 1), ms)
    return () => window.clearInterval(id)
  }, [active, ms])
  return n
}

function Glyph({
  label,
  className,
  children,
  divRef,
  viewBox = '0 0 280 158',
}: {
  label: string
  className?: string
  children: React.ReactNode
  divRef: React.Ref<HTMLDivElement>
  viewBox?: string
}) {
  return (
    <div ref={divRef} className={cn('relative', className)}>
      <svg viewBox={viewBox} role="img" aria-label={label} className="h-auto w-full overflow-visible">
        {children}
      </svg>
    </div>
  )
}

function Logo({ x, y, s }: { x: number; y: number; s: number }) {
  return (
    <g transform={`translate(${x} ${y}) scale(${s})`} fill="var(--foreground)">
      <rect width="233.4" height="233.4" />
      <rect y="350.1" width="233.4" height="700.21" />
      <rect x="350.1" width="233.4" height="233.4" />
      <rect x="350.1" y="350.1" width="233.4" height="700.21" />
      <rect x="700.21" width="233.4" height="233.4" />
      <rect x="700.21" y="350.1" width="233.4" height="700.21" />
    </g>
  )
}

function Chip({ x, y, kind }: { x: number; y: number; kind: string }) {
  return (
    <>
      <rect x={x} y={y} width={18} height={18} rx={5} fill="var(--faint)" stroke="var(--line)" />
      <text x={x + 9} y={y + 12.2} textAnchor="middle" fontSize={8.5} className="fill-muted-foreground font-mono">
        {kind}
      </text>
    </>
  )
}

const WORKER_KINDS = [
  { kind: 'ts', label: 'orders-api', what: 'an API' },
  { kind: 'py', label: 'report.py', what: 'a Python script' },
  { kind: 'rs', label: 'resize', what: 'a Rust binary' },
  { kind: 'js', label: 'browser tab', what: 'a browser tab' },
  { kind: 'ai', label: 'agent', what: 'an agent' },
  { kind: 'q', label: 'queue', what: 'the queue' },
]

const W = { x: 62, y: 64, hw: 56, hh: 18 }
const E = { x: 234, y: 64, hw: 40, hh: 26 }

/** Worker glyph: processes of different kinds plug into the engine over one WebSocket, with a heartbeat on the line. */
export function WorkerGlyph({ className }: { className?: string }) {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const n = useCycle(2800, active)
  const w = WORKER_KINDS[n % WORKER_KINDS.length]
  const x1 = W.x + W.hw
  const x2 = E.x - E.hw

  return (
    <Glyph
      divRef={ref}
      className={className}
      label="A worker process, such as an API, a Python script, a Rust binary, a browser tab, an agent, or the queue, connected to the engine over one WebSocket."
    >
      {/* Engine */}
      <rect
        x={E.x - E.hw}
        y={E.y - E.hh}
        width={E.hw * 2}
        height={E.hh * 2}
        rx={12}
        fill="var(--node)"
        stroke="var(--line-strong)"
      />
      <Logo x={E.x - 7.5} y={E.y - 17} s={0.016} />
      <text
        x={E.x}
        y={E.y + 15}
        textAnchor="middle"
        fontSize={9}
        letterSpacing="0.12em"
        className="fill-muted-foreground font-mono"
      >
        ENGINE
      </text>

      <text x={(x1 + x2) / 2} y={E.y - 8} textAnchor="middle" fontSize={9} className="fill-muted-foreground font-mono">
        ws
      </text>
      <line x1={x1} y1={W.y} x2={x2} y2={W.y} stroke="var(--line)" strokeDasharray="2 4" />

      <AnimatePresence initial={false}>
        <motion.g key={n} exit={{ opacity: 0 }} transition={{ duration: 0.25 }}>
          {/* Connection snaps in */}
          <motion.line
            x1={x1}
            y1={W.y}
            x2={x2}
            y2={W.y}
            stroke="var(--line-strong)"
            initial={active ? { pathLength: 0 } : false}
            animate={{ pathLength: 1 }}
            transition={{ duration: 0.35, delay: 0.45, ease: 'easeOut' }}
          />
          <motion.circle
            cx={x2}
            cy={W.y}
            r={2.5}
            fill="var(--foreground)"
            initial={active ? { opacity: 0, scale: 0 } : false}
            animate={{ opacity: 1, scale: 1 }}
            transition={{ duration: 0.25, delay: 0.8 }}
            style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
          />
          {/* Heartbeat */}
          {active
            ? [1.05, 1.85].map((d) => (
                <motion.circle
                  key={d}
                  r={2.5}
                  cy={W.y}
                  fill="var(--foreground)"
                  initial={{ cx: x1, opacity: 0 }}
                  animate={{ cx: x2, opacity: [0, 1, 1, 0] }}
                  transition={{ duration: 0.55, delay: d, ease: 'easeInOut' }}
                />
              ))
            : null}
          {/* Worker */}
          <motion.g
            initial={active ? { opacity: 0, x: -14 } : false}
            animate={{ opacity: 1, x: 0 }}
            transition={{ duration: 0.45, ease }}
          >
            <rect
              x={W.x - W.hw}
              y={W.y - W.hh}
              width={W.hw * 2}
              height={W.hh * 2}
              rx={10}
              fill="var(--node)"
              stroke="var(--line-strong)"
            />
            <Chip x={W.x - W.hw + 8} y={W.y - 9} kind={w.kind} />
            <text x={W.x - W.hw + 33} y={W.y + 4} fontSize={11} className="fill-foreground font-mono">
              {w.label}
            </text>
            <circle cx={x1} cy={W.y} r={2.5} fill="var(--node)" stroke="var(--foreground)" />
            <text x={W.x} y={W.y + 42} textAnchor="middle" fontSize={10} className="fill-muted-foreground font-mono">
              {w.what}
            </text>
          </motion.g>
          {/* Connected status */}
          <motion.circle
            cx={E.x + E.hw - 10}
            cy={E.y - E.hh + 10}
            r={2.5}
            fill="var(--ok)"
            initial={active ? { opacity: 0 } : false}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.2, delay: 0.85 }}
          />
        </motion.g>
      </AnimatePresence>

      {/* Kind index */}
      {WORKER_KINDS.map((k, i) => (
        <circle
          key={k.kind}
          cx={140 - (WORKER_KINDS.length - 1) * 4 + i * 8}
          cy={146}
          r={1.75}
          fill={i === n % WORKER_KINDS.length ? 'var(--foreground)' : 'var(--line-strong)'}
          style={{ transition: 'fill 300ms ease' }}
        />
      ))}
    </Glyph>
  )
}

const FN = { x: 80, y: 44, w: 120, h: 32 }
const CARD = { y: 98, w: 130, h: 56 }
const IN_ROWS = [
  { key: 'order_id', type: 'string' },
  { key: 'amount', type: 'number' },
]
const OUT_ROWS = [
  { key: 'refund_id', type: 'string' },
  { key: 'status', type: 'string' },
]

function SchemaCard({
  x,
  title,
  rows,
  delay,
  pulse,
}: {
  x: number
  title: string
  rows: { key: string; type: string }[]
  delay: number
  pulse: boolean
}) {
  return (
    <g>
      <rect x={x} y={CARD.y} width={CARD.w} height={CARD.h} rx={10} fill="var(--node)" stroke="var(--line-strong)" />
      <text x={x + 10} y={CARD.y + 14} fontSize={8.5} letterSpacing="0.1em" className="fill-muted-foreground font-mono">
        {title}
      </text>
      {rows.map((r, i) => {
        const y = CARD.y + 30 + i * 15
        return (
          <g key={r.key}>
            {pulse ? (
              <motion.rect
                x={x + 4}
                y={y - 10}
                width={CARD.w - 8}
                height={14}
                rx={4}
                fill="var(--foreground)"
                initial={{ fillOpacity: 0 }}
                animate={{ fillOpacity: [0, 0.1, 0] }}
                transition={{ duration: 0.8, delay: delay + i * 0.15 }}
              />
            ) : null}
            <text x={x + 10} y={y} fontSize={9.5} className="fill-foreground font-mono">
              {r.key}
            </text>
            <text x={x + CARD.w - 10} y={y} textAnchor="end" fontSize={9} className="fill-muted-foreground font-mono">
              {r.type}
            </text>
          </g>
        )
      })}
    </g>
  )
}

/** Function glyph: a call arrives by name at `orders::refund`, input is checked against its schema, a typed result leaves. */
export function FunctionGlyph({ className }: { className?: string }) {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const n = useCycle(4200, active)
  const cx = FN.x + FN.w / 2
  const cy = FN.y + FN.h / 2
  const inX = 4 + CARD.w / 2
  const outX = 146 + CARD.w / 2

  return (
    <Glyph
      divRef={ref}
      className={className}
      label="A function named orders::refund with a JSON schema for its input (order_id, amount) and output (refund_id, status). A caller reaches it by name."
    >
      {/* Caller, by name */}
      <rect
        x={cx - 70}
        y={4}
        width={140}
        height={20}
        rx={10}
        fill="var(--faint)"
        stroke="var(--line-strong)"
        strokeDasharray="3 3"
      />
      <text x={cx} y={17.5} textAnchor="middle" fontSize={9.5} className="fill-muted-foreground font-mono">
        call(&apos;orders::refund&apos;)
      </text>
      <line x1={cx} y1={24} x2={cx} y2={FN.y} stroke="var(--line-strong)" />
      <path d={`M${inX} ${CARD.y} V${cy} H${FN.x}`} fill="none" stroke="var(--line-strong)" />
      <path d={`M${FN.x + FN.w} ${cy} H${outX} V${CARD.y}`} fill="none" stroke="var(--line-strong)" />

      <SchemaCard key={`in-${n}`} x={4} title="INPUT" rows={IN_ROWS} delay={0.45} pulse={active} />
      <SchemaCard key={`out-${n}`} x={146} title="OUTPUT" rows={OUT_ROWS} delay={1.95} pulse={active} />

      <rect x={FN.x} y={FN.y} width={FN.w} height={FN.h} rx={10} fill="var(--node)" stroke="var(--foreground)" />
      <text x={cx} y={cy + 3.8} textAnchor="middle" fontSize={11} className="fill-foreground font-mono">
        orders::refund
      </text>

      <AnimatePresence>
        {active ? (
          <motion.g key={n} exit={{ opacity: 0 }} transition={{ duration: 0.3 }}>
            {/* call arrives by name */}
            <motion.circle
              r={3}
              cx={cx}
              fill="var(--foreground)"
              initial={{ cy: 24, opacity: 0 }}
              animate={{ cy: FN.y, opacity: [0, 1, 1, 0] }}
              transition={{ duration: 0.4, ease: 'easeInOut' }}
            />
            {/* input, checked against its schema, flows in */}
            <motion.circle
              r={3}
              fill="var(--foreground)"
              initial={{ cx: inX, cy: CARD.y, opacity: 0 }}
              animate={{ cx: [inX, inX, FN.x], cy: [CARD.y, cy, cy], opacity: [0, 1, 0] }}
              transition={{ duration: 0.5, delay: 0.9, ease: 'easeInOut', times: [0, 0.78, 1] }}
            />
            {/* work runs */}
            <motion.rect
              x={FN.x + 12}
              y={FN.y + FN.h - 6}
              height={1.5}
              rx={0.75}
              fill="var(--foreground)"
              initial={{ width: 0, opacity: 1 }}
              animate={{ width: FN.w - 24, opacity: [1, 1, 0] }}
              transition={{
                width: { duration: 0.5, delay: 1.35 },
                opacity: { duration: 0.8, delay: 1.35, times: [0, 0.7, 1] },
              }}
            />
            {/* result leaves */}
            <motion.circle
              r={3}
              fill="var(--foreground)"
              initial={{ cx: FN.x + FN.w, cy, opacity: 0 }}
              animate={{ cx: [FN.x + FN.w, outX, outX], cy: [cy, cy, CARD.y], opacity: [0, 1, 0] }}
              transition={{ duration: 0.5, delay: 1.75, ease: 'easeInOut', times: [0, 0.22, 1] }}
            />
            <motion.circle
              cx={146 + CARD.w - 12}
              cy={CARD.y + 11}
              r={2.5}
              fill="var(--ok)"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 0.2, delay: 2.25 }}
            />
          </motion.g>
        ) : null}
      </AnimatePresence>
    </Glyph>
  )
}

const SOURCES = [
  { label: 'direct call', detail: 'iii.trigger(…)' },
  { label: 'http', detail: 'POST /refunds' },
  { label: 'cron', detail: '0 0 9 * * * *' },
  { label: 'queue message', detail: 'topic refunds' },
  { label: 'state change', detail: 'orders.status set' },
  { label: 'custom event', detail: 'support.escalated' },
]

const T_FN = { x: 172, y: 58, w: 104, h: 32 }
const srcY = (i: number) => 17 + i * 25

function curve(i: number) {
  const y = srcY(i)
  const ty = T_FN.y + T_FN.h / 2
  return {
    d: `M100 ${y} C136 ${y} 136 ${ty} ${T_FN.x} ${ty}`,
    at: (t: number) => {
      const u = 1 - t
      const x = u ** 3 * 100 + 3 * u * u * t * 136 + 3 * u * t * t * 136 + t ** 3 * T_FN.x
      const yy = u ** 3 * y + 3 * u * u * t * y + 3 * u * t * t * ty + t ** 3 * ty
      return { x, y: yy }
    },
  }
}

/** Trigger glyph: six kinds of trigger (call, HTTP, cron, queue, state, custom event) fire the same function one at a time. */
export function TriggerGlyph({ className }: { className?: string }) {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const n = useCycle(1500, active)
  const current = active ? n % SOURCES.length : 1
  const c = curve(current)
  const samples = Array.from({ length: 9 }, (_, i) => c.at(i / 8))

  return (
    <Glyph
      divRef={ref}
      className={className}
      label="Six kinds of trigger, a direct call, an HTTP request, a cron schedule, a queue message, a state change, and a custom event, each run the function orders::refund."
    >
      {SOURCES.map((s, i) => (
        <path key={s.label} d={curve(i).d} fill="none" stroke="var(--line)" />
      ))}
      <AnimatePresence>
        <motion.g key={`${current}-${n}`} exit={{ opacity: 0 }} transition={{ duration: 0.3 }}>
          <motion.path
            d={c.d}
            fill="none"
            stroke="var(--foreground)"
            strokeWidth={1.25}
            initial={active ? { pathLength: 0 } : false}
            animate={{ pathLength: 1 }}
            transition={{ duration: 0.5, ease: 'easeInOut' }}
          />
          {active ? (
            <motion.circle
              r={3}
              fill="var(--foreground)"
              initial={{ cx: samples[0].x, cy: samples[0].y, opacity: 0 }}
              animate={{
                cx: samples.map((p) => p.x),
                cy: samples.map((p) => p.y),
                opacity: [0, 1, 1, 1, 1, 1, 1, 1, 0],
              }}
              transition={{ duration: 0.5, ease: 'linear' }}
            />
          ) : null}
        </motion.g>
      </AnimatePresence>

      {SOURCES.map((s, i) => {
        const y = srcY(i)
        const on = i === current
        return (
          <g key={s.label}>
            <rect
              x={4}
              y={y - 9}
              width={96}
              height={18}
              rx={5}
              fill="var(--node)"
              stroke={on ? 'var(--foreground)' : 'var(--line-strong)'}
              style={{ transition: 'stroke 250ms ease' }}
            />
            <circle
              cx={13}
              cy={y}
              r={2}
              fill={on ? 'var(--foreground)' : 'var(--line-strong)'}
              style={{ transition: 'fill 250ms ease' }}
            />
            <text
              x={21}
              y={y + 3.2}
              fontSize={9.5}
              className={cn('font-mono', on ? 'fill-foreground' : 'fill-muted-foreground')}
              style={{ transition: 'fill 250ms ease' }}
            >
              {s.label}
            </text>
          </g>
        )
      })}

      <rect
        x={T_FN.x}
        y={T_FN.y}
        width={T_FN.w}
        height={T_FN.h}
        rx={10}
        fill="var(--node)"
        stroke="var(--foreground)"
      />
      <text
        x={T_FN.x + T_FN.w / 2}
        y={T_FN.y + T_FN.h / 2 + 3.8}
        textAnchor="middle"
        fontSize={11}
        className="fill-foreground font-mono"
      >
        orders::refund
      </text>
      <AnimatePresence>
        {active ? (
          <motion.rect
            key={`pulse-${n}`}
            x={T_FN.x}
            y={T_FN.y}
            width={T_FN.w}
            height={T_FN.h}
            rx={10}
            fill="none"
            stroke="var(--foreground)"
            style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
            initial={{ opacity: 0.5, scale: 1 }}
            animate={{ opacity: 0, scale: 1.12 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.7, delay: 0.5, ease: 'easeOut' }}
          />
        ) : null}
      </AnimatePresence>
      <AnimatePresence mode="wait" initial={false}>
        <motion.text
          key={current}
          x={T_FN.x + T_FN.w / 2}
          y={T_FN.y + T_FN.h + 20}
          textAnchor="middle"
          fontSize={9}
          className="fill-muted-foreground font-mono"
          initial={{ opacity: 0, y: T_FN.y + T_FN.h + 24 }}
          animate={{ opacity: 1, y: T_FN.y + T_FN.h + 20 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.3, ease }}
        >
          {SOURCES[current].detail}
        </motion.text>
      </AnimatePresence>
    </Glyph>
  )
}
