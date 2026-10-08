'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'

import { cn } from '@/lib/utils'
import { useGraphicLoop } from './use-graphic-loop'

const ease = [0.22, 1, 0.36, 1] as const

/** Counts up every `ms` while the graphic is on screen and motion is allowed. */
function useTicker(ms: number) {
  const loop = useGraphicLoop<HTMLDivElement>()
  const [step, setStep] = useState(0)
  useEffect(() => {
    if (!loop.active) return
    const id = window.setInterval(() => setStep((s) => s + 1), ms)
    return () => window.clearInterval(id)
  }, [loop.active, ms])
  return { ...loop, step }
}

function Label({
  x,
  y,
  children,
  anchor,
  className,
}: {
  x: number
  y: number
  children: React.ReactNode
  anchor?: 'start' | 'middle' | 'end'
  className?: string
}) {
  return (
    <text x={x} y={y} textAnchor={anchor} className={cn('fill-muted-foreground font-mono', className)} fontSize={9.5}>
      {children}
    </text>
  )
}

function Message({ fill = 'var(--foreground)' }: { fill?: string }) {
  return <rect x={-6} y={-4.5} width={12} height={9} rx={2.5} fill={fill} />
}

/* Retries with exponential backoff, then the dead-letter queue */

const R_LANE = 56
const R_DLQ = 108
const ATTEMPTS = [70, 100, 160, 280]
const R_START = 16
const R_CYCLE = 8000

// Absolute times (s) inside one cycle.
const R_HIT = [0.5, 1.3, 2.5, 4.4]
const R_DROP = 5.3
const R_PRESS = 6.1

/** A failing message retried after 1s, 2s, 4s, then parked in the dead-letter queue to retry or discard. */
export function QueueRetry({ className }: { className?: string }) {
  const { ref, active, step } = useTicker(R_CYCLE)
  const retry = step % 2 === 0
  const key = `retry-${step}`

  const xs = [R_START, 70, 70, 100, 100, 160, 160, 280, 280, 280, 280]
  const ys = [R_LANE, R_LANE, R_LANE, R_LANE, R_LANE, R_LANE, R_LANE, R_LANE, R_LANE, R_DLQ, R_DLQ]
  const at = [0, 0.5, 0.8, 1.3, 1.6, 2.5, 2.8, 4.4, 4.8, R_DROP, R_PRESS + 0.3]
  if (retry) {
    xs.push(R_START, R_START)
    ys.push(R_DLQ, R_LANE)
    at.push(7.3, 7.8)
  }
  const total = at[at.length - 1]
  const times = at.map((v) => v / total)

  return (
    <div ref={ref} className={className}>
      <svg
        viewBox="0 0 320 148"
        role="img"
        aria-label="A message fails and is retried after 1 second, then 2 seconds, then 4 seconds. After the last failure it moves to a dead-letter queue, where it can be retried or discarded."
        className="h-auto w-full"
      >
        <Label x={R_START} y={22}>
          attempts
        </Label>
        <line x1={R_START} y1={R_LANE} x2={304} y2={R_LANE} stroke="var(--line-strong)" />

        {/* Backoff gaps */}
        {[0, 1, 2].map((i) => {
          const a = ATTEMPTS[i] + 7
          const b = ATTEMPTS[i + 1] - 7
          const d = `M ${a} 42 V 38 H ${b} V 42`
          return (
            <g key={i}>
              {active ? (
                <motion.path
                  key={`${key}-gap-${i}`}
                  d={d}
                  fill="none"
                  stroke="var(--warn)"
                  initial={{ pathLength: 0, opacity: 0.8 }}
                  animate={{ pathLength: 1 }}
                  transition={{ duration: R_HIT[i + 1] - R_HIT[i] - 0.3, delay: R_HIT[i] + 0.3, ease: 'linear' }}
                />
              ) : (
                <path d={d} fill="none" stroke="var(--warn)" opacity={0.8} />
              )}
              <text x={(a + b) / 2} y={32} textAnchor="middle" className="font-mono" fill="var(--warn)" fontSize={9.5}>
                {2 ** i}s
              </text>
            </g>
          )
        })}

        {/* Attempt markers */}
        {ATTEMPTS.map((x, i) => {
          const color = i === ATTEMPTS.length - 1 ? 'var(--fail)' : 'var(--warn)'
          return (
            <g key={x}>
              <circle cx={x} cy={R_LANE} r={4} fill="var(--node)" stroke="var(--line-strong)" />
              {active ? (
                <motion.circle
                  key={`${key}-hit-${i}`}
                  cx={x}
                  cy={R_LANE}
                  r={4}
                  fill={color}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={{ duration: 0.2, delay: R_HIT[i] }}
                />
              ) : (
                <circle cx={x} cy={R_LANE} r={4} fill={color} />
              )}
              <Label x={x} y={R_LANE + 18} anchor="middle">
                {i + 1}
              </Label>
            </g>
          )
        })}

        {/* Dead-letter lane */}
        <line x1={278} y1={R_LANE + 4} x2={278} y2={R_DLQ} stroke="var(--line-strong)" strokeDasharray="3 4" />
        <Label x={R_START} y={R_DLQ - 8}>
          dead-letter
        </Label>
        <line x1={R_START} y1={R_DLQ} x2={304} y2={R_DLQ} stroke="var(--line-strong)" strokeDasharray="3 4" />

        {/* Retry / discard */}
        {[
          { label: 'retry', x: 196, w: 46, on: retry },
          { label: 'discard', x: 248, w: 56, on: !retry },
        ].map((c) => (
          <g key={c.label}>
            <rect x={c.x} y={120} width={c.w} height={20} rx={6} fill="var(--node)" stroke="var(--line-strong)" />
            {active && c.on ? (
              <motion.rect
                key={`${key}-press`}
                x={c.x}
                y={120}
                width={c.w}
                height={20}
                rx={6}
                fill="var(--faint)"
                stroke="var(--foreground)"
                initial={{ opacity: 0 }}
                animate={{ opacity: [0, 1, 1, 0] }}
                transition={{ duration: 1, delay: R_PRESS - 0.2, times: [0, 0.2, 0.7, 1] }}
              />
            ) : null}
            <Label x={c.x + c.w / 2} y={133.5} anchor="middle" className="fill-foreground">
              {c.label}
            </Label>
          </g>
        ))}

        {/* The message */}
        {active ? (
          <motion.g
            key={key}
            initial={{ x: R_START, y: R_LANE, opacity: 0 }}
            animate={{
              x: xs,
              y: ys,
              opacity: retry ? 1 : [0, 1, 1, 0],
            }}
            transition={{
              x: { duration: total, times, ease: 'easeInOut' },
              y: { duration: total, times, ease: 'easeInOut' },
              opacity: retry ? { duration: 0.3 } : { duration: total, times: [0, 0.05, 0.93, 1] },
            }}
          >
            <Message />
            <motion.circle
              cx={12}
              cy={0}
              r={2.5}
              fill="var(--fail)"
              initial={{ opacity: 0 }}
              animate={{ opacity: retry ? [0, 1, 1, 0] : [0, 1, 1] }}
              transition={{
                duration: total - R_DROP,
                delay: R_DROP,
                times: retry ? [0, 0.1, 0.4, 0.5] : [0, 0.1, 1],
              }}
            />
          </motion.g>
        ) : (
          <g transform={`translate(280 ${R_DLQ})`}>
            <Message />
            <circle cx={12} cy={0} r={2.5} fill="var(--fail)" />
          </g>
        )}
      </svg>
    </div>
  )
}

/* Ordered queue: one message at a time per key */

const O_TICK = 700
const O_SLOTS = [212, 186, 160, 134, 108, 82]
const O_RUN_X = 272
const O_LANES = [
  { key: 'billing', y: 52, period: 3 },
  { key: 'growth', y: 112, period: 2 },
]

/** Two message groups keyed by team: each key runs strictly one after another; different keys run side by side. */
export function QueueOrdered({ className }: { className?: string }) {
  const { ref, active, step } = useTicker(O_TICK)

  return (
    <div ref={ref} className={className}>
      <svg
        viewBox="0 0 320 148"
        role="img"
        aria-label="An ordered queue with two keys, billing and growth. Messages for the same key run one at a time, in order; the two keys progress independently."
        className="h-auto w-full"
      >
        {O_LANES.map((lane) => {
          const current = Math.floor(step / lane.period)
          const ids = [0, 1, 2, 3, 4, 5, 6].map((n) => current + n)
          return (
            <g key={lane.key}>
              <Label x={16} y={lane.y - 22}>
                key: <tspan className="fill-foreground">{lane.key}</tspan>
              </Label>
              <line x1={16} y1={lane.y} x2={240} y2={lane.y} stroke="var(--line-strong)" />
              <rect
                x={240}
                y={lane.y - 16}
                width={64}
                height={32}
                rx={8}
                fill="var(--node)"
                stroke="var(--line-strong)"
              />
              <line
                x1={250}
                y1={lane.y + 10}
                x2={294}
                y2={lane.y + 10}
                stroke="var(--line)"
                strokeWidth={2}
                strokeLinecap="round"
              />
              {active ? (
                <motion.line
                  key={`${lane.key}-progress-${current}`}
                  x1={250}
                  y1={lane.y + 10}
                  x2={294}
                  y2={lane.y + 10}
                  stroke="var(--foreground)"
                  strokeWidth={2}
                  strokeLinecap="round"
                  initial={{ pathLength: 0 }}
                  animate={{ pathLength: 1 }}
                  transition={{ duration: (lane.period * O_TICK) / 1000 - 0.15, ease: 'linear' }}
                />
              ) : (
                <line
                  x1={250}
                  y1={lane.y + 10}
                  x2={272}
                  y2={lane.y + 10}
                  stroke="var(--foreground)"
                  strokeWidth={2}
                  strokeLinecap="round"
                />
              )}
              {active && current > 0 ? (
                <motion.circle
                  key={`${lane.key}-ok-${current}`}
                  cx={296}
                  cy={lane.y - 8}
                  r={2.5}
                  fill="var(--ok)"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: [0, 1, 0] }}
                  transition={{ duration: 0.9, times: [0, 0.2, 1] }}
                />
              ) : null}
              <AnimatePresence initial={false}>
                {ids.map((id) => {
                  const slot = id - current
                  const x = slot === 0 ? O_RUN_X : O_SLOTS[slot - 1]
                  const y = slot === 0 ? lane.y - 3 : lane.y
                  return (
                    <motion.g
                      key={id}
                      initial={{ x: O_SLOTS[O_SLOTS.length - 1] - 26, y: lane.y, opacity: 0 }}
                      animate={{ x, y, opacity: 1 }}
                      exit={{ x: O_RUN_X + 14, opacity: 0, transition: { duration: 0.3, ease } }}
                      transition={{ duration: 0.45, ease }}
                    >
                      <rect
                        x={-10}
                        y={-7}
                        width={20}
                        height={14}
                        rx={3.5}
                        fill={slot === 0 ? 'var(--foreground)' : 'var(--node)'}
                        stroke={slot === 0 ? 'none' : 'var(--line-strong)'}
                      />
                      <text
                        x={0}
                        y={3}
                        textAnchor="middle"
                        className={cn('font-mono', slot === 0 ? 'fill-background' : 'fill-muted-foreground')}
                        fontSize={8.5}
                      >
                        {id + 1}
                      </text>
                    </motion.g>
                  )
                })}
              </AnimatePresence>
            </g>
          )
        })}
        <Label x={O_RUN_X} y={O_LANES[0].y - 22} anchor="middle">
          running
        </Label>
      </svg>
    </div>
  )
}

/* Condition function: decides whether a message runs */

const C_LANE = 60
const C_SKIP = 116
const C_GATE = 132
const C_NODE = { x: 184, w: 122 }
const C_TICK = 1700
const C_PATTERN = [true, true, false, true, false]

/** Messages meet a condition function; `true` runs the handler, `false` skips the message. */
export function QueueCondition({ className }: { className?: string }) {
  const { ref, active, step } = useTicker(C_TICK)
  const passes = (k: number) => C_PATTERN[k % C_PATTERN.length]
  const last = passes(step)

  return (
    <div ref={ref} className={className}>
      <svg
        viewBox="0 0 320 148"
        role="img"
        aria-label="Messages reach a condition function. When it returns true the message runs reports::generate; when it returns false the message is skipped."
        className="h-auto w-full"
      >
        <line x1={16} y1={C_LANE} x2={C_GATE - 14} y2={C_LANE} stroke="var(--line-strong)" />
        <line x1={C_GATE + 14} y1={C_LANE} x2={C_NODE.x} y2={C_LANE} stroke="var(--line-strong)" />
        <path
          d={`M ${C_GATE} ${C_LANE + 14} V ${C_SKIP - 6} Q ${C_GATE} ${C_SKIP} ${C_GATE + 6} ${C_SKIP} H ${C_NODE.x}`}
          fill="none"
          stroke="var(--line-strong)"
          strokeDasharray="3 4"
        />

        {/* Gate */}
        <Label x={C_GATE} y={C_LANE - 24} anchor="middle">
          condition
        </Label>
        <path
          d={`M ${C_GATE} ${C_LANE - 14} L ${C_GATE + 14} ${C_LANE} L ${C_GATE} ${C_LANE + 14} L ${C_GATE - 14} ${C_LANE} Z`}
          fill="var(--node)"
          stroke="var(--line-strong)"
          strokeLinejoin="round"
        />
        <text x={C_GATE} y={C_LANE + 3.5} textAnchor="middle" className="fill-muted-foreground font-mono" fontSize={9}>
          ?
        </text>
        {active ? (
          <motion.path
            key={`gate-${step}`}
            d={`M ${C_GATE} ${C_LANE - 14} L ${C_GATE + 14} ${C_LANE} L ${C_GATE} ${C_LANE + 14} L ${C_GATE - 14} ${C_LANE} Z`}
            fill="none"
            stroke="var(--foreground)"
            strokeLinejoin="round"
            initial={{ opacity: 0 }}
            animate={{ opacity: [0, 1, 0] }}
            transition={{ duration: 0.7, delay: 0.75, times: [0, 0.3, 1] }}
          />
        ) : null}

        {/* Branch labels */}
        <BranchLabel
          x={(C_GATE + 14 + C_NODE.x) / 2}
          y={C_LANE - 8}
          anchor="middle"
          on={!active || last}
          flash={active && last}
          runKey={step}
        >
          true
        </BranchLabel>
        <BranchLabel x={C_GATE + 8} y={C_LANE + 36} on={!active ? false : !last} flash={active && !last} runKey={step}>
          false
        </BranchLabel>

        {/* Handler and skipped */}
        <rect
          x={C_NODE.x}
          y={C_LANE - 15}
          width={C_NODE.w}
          height={30}
          rx={8}
          fill="var(--node)"
          stroke="var(--line-strong)"
        />
        {active && last ? (
          <motion.rect
            key={`node-${step}`}
            x={C_NODE.x}
            y={C_LANE - 15}
            width={C_NODE.w}
            height={30}
            rx={8}
            fill="none"
            stroke="var(--foreground)"
            initial={{ opacity: 0 }}
            animate={{ opacity: [0, 1, 0] }}
            transition={{ duration: 0.9, delay: 1.3, times: [0, 0.25, 1] }}
          />
        ) : null}
        <text x={C_NODE.x + 10} y={C_LANE + 3.5} className="fill-foreground font-mono" fontSize={10}>
          reports::generate
        </text>
        <Label x={C_NODE.x + 10} y={C_SKIP + 3.5}>
          skipped
        </Label>

        {/* Messages */}
        {active ? (
          [step - 1, step].map((k) => (k < 0 ? null : <ConditionMessage key={k} pass={passes(k)} />))
        ) : (
          <g transform={`translate(${C_GATE - 50} ${C_LANE})`}>
            <Message />
          </g>
        )}
      </svg>
    </div>
  )
}

function BranchLabel({
  x,
  y,
  anchor,
  on,
  flash,
  runKey,
  children,
}: {
  x: number
  y: number
  anchor?: 'start' | 'middle' | 'end'
  on: boolean
  flash: boolean
  runKey: number
  children: React.ReactNode
}) {
  return (
    <g>
      <Label x={x} y={y} anchor={anchor}>
        {children}
      </Label>
      {flash ? (
        <motion.text
          key={runKey}
          x={x}
          y={y}
          textAnchor={anchor}
          className="fill-foreground font-mono"
          fontSize={9.5}
          initial={{ opacity: 0 }}
          animate={{ opacity: [0, 1, 1, 0] }}
          transition={{ duration: 1.3, delay: 0.8, times: [0, 0.15, 0.75, 1] }}
        >
          {children}
        </motion.text>
      ) : on ? (
        <text x={x} y={y} textAnchor={anchor} className="fill-foreground font-mono" fontSize={9.5}>
          {children}
        </text>
      ) : null}
    </g>
  )
}

function ConditionMessage({ pass }: { pass: boolean }) {
  const xs = pass
    ? [16, C_GATE - 22, C_GATE - 22, C_NODE.x - 10, C_NODE.x - 10]
    : [16, C_GATE - 22, C_GATE - 22, C_GATE, C_GATE, C_NODE.x - 10]
  const ys = pass ? [C_LANE, C_LANE, C_LANE, C_LANE, C_LANE] : [C_LANE, C_LANE, C_LANE, C_LANE, C_SKIP, C_SKIP]
  const times = pass ? [0, 0.42, 0.56, 0.86, 1] : [0, 0.36, 0.48, 0.58, 0.8, 1]
  const duration = pass ? 1.55 : 1.85
  return (
    <motion.g
      initial={{ x: 16, y: C_LANE, opacity: 0 }}
      animate={{ x: xs, y: ys, opacity: pass ? [0, 1, 1, 1, 0] : [0, 1, 1, 1, 0.45, 0] }}
      transition={{
        x: { duration, times, ease: 'easeInOut' },
        y: { duration, times, ease: 'easeInOut' },
        opacity: { duration, times },
      }}
    >
      <Message />
    </motion.g>
  )
}
