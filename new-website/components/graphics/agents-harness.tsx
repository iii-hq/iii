'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { easeInOut, easeOut } from '@/lib/motion'

import { cn } from '@/lib/utils'
import { useGraphicLoop } from './use-graphic-loop'

type Point = { x: number; y: number }
type Box = { x: number; y: number; w: number; h: number }
type Source = 'harness' | 'sub1' | 'sub2'
type RowId = 'orders::get' | 'email::send' | 'db::drop' | 'payments::payout' | 'orders::refund'
type Verdict = 'allowed' | 'denied' | 'waiting'

const ease = easeOut
const STEP_MS = 2400
const TRAVEL = 0.9

const MSG: Box = { x: 12, y: 12, w: 376, h: 50 }
const HARNESS: Box = { x: 12, y: 101, w: 124, h: 36 }
const SUBS: Record<'sub1' | 'sub2', Box & { branch: string }> = {
  sub1: { x: 42, y: 170, w: 94, h: 40, branch: 'agent/email' },
  sub2: { x: 42, y: 222, w: 94, h: 40, branch: 'agent/ledger' },
}
const BUS_X = 152
const GATE = { x: 166, w: 6, y: 100, h: 246 }
const ROW_X = 196
const ROW_W = 192
const ROW_H = 30
const ROWS: RowId[] = ['orders::get', 'email::send', 'db::drop', 'payments::payout', 'orders::refund']
const rowY = (id: RowId) => 104 + ROWS.indexOf(id) * 38
const rowCy = (id: RowId) => rowY(id) + ROW_H / 2
const APPROVAL: Box = { x: ROW_X, y: 306, w: ROW_W, h: 40 }
const APPROVAL_X = ROW_X + ROW_W / 2
const TRACE_Y = 366

const sourceCy: Record<Source, number> = {
  harness: HARNESS.y + HARNESS.h / 2,
  sub1: SUBS.sub1.y + SUBS.sub1.h / 2,
  sub2: SUBS.sub2.y + SUBS.sub2.h / 2,
}

type Step = {
  caption: string
  call?: { from: Source; to: RowId; verdict: Verdict }
  send?: boolean
  approve?: boolean
  spawn?: boolean
}

const SCRIPT: Step[] = [
  { caption: 'harness::send → new session, allowed to call orders::* and email::send', send: true },
  { caption: 'orders::get → allowed by orders::*', call: { from: 'harness', to: 'orders::get', verdict: 'allowed' } },
  { caption: 'db::drop → denied, not on the allow list', call: { from: 'harness', to: 'db::drop', verdict: 'denied' } },
  {
    caption: 'orders::refund → sensitive, held for human approval',
    call: { from: 'harness', to: 'orders::refund', verdict: 'waiting' },
  },
  { caption: 'approved → orders::refund runs', approve: true },
  { caption: 'spawned 2 sub-agents, each on its own git worktree', spawn: true },
  { caption: 'sub-agent → email::send → allowed', call: { from: 'sub1', to: 'email::send', verdict: 'allowed' } },
  {
    caption: 'sub-agent → payments::payout → denied',
    call: { from: 'sub2', to: 'payments::payout', verdict: 'denied' },
  },
  { caption: 'every run lands in one trace: 8 spans from 3 agents' },
]
const LAST = SCRIPT.length - 1

/** Status of each function row after step `s` has played. */
function rowStatus(id: RowId, s: number): 'idle' | 'allowed' | 'denied' | 'waiting' {
  switch (id) {
    case 'orders::get':
      return s >= 1 ? 'allowed' : 'idle'
    case 'db::drop':
      return s >= 2 ? 'denied' : 'idle'
    case 'orders::refund':
      return s >= 4 ? 'allowed' : s === 3 ? 'waiting' : 'idle'
    case 'email::send':
      return s >= 6 ? 'allowed' : 'idle'
    case 'payments::payout':
      return s >= 7 ? 'denied' : 'idle'
  }
}

type Span = { start: number; end: number; at: number; fail?: boolean; waitUntil?: number }

/** Trace waterfall, one bar per span, in tree order. Times are in step units. */
const SPANS: Span[] = [
  { start: 0.1, end: 8.7, at: 0 },
  { start: 1.3, end: 1.9, at: 1 },
  { start: 2.3, end: 2.6, at: 2, fail: true },
  { start: 3.3, end: 4.6, at: 3, waitUntil: 4 },
  { start: 5.2, end: 8.3, at: 5 },
  { start: 6.3, end: 7.0, at: 6 },
  { start: 5.3, end: 8.1, at: 5 },
  { start: 7.3, end: 7.6, at: 7, fail: true },
]
const tx = (t: number) => 12 + (t / 9) * 376

function callPoints(from: Source, to: RowId, verdict: Verdict): Point[] {
  const sy = sourceCy[from]
  const ty = rowCy(to)
  const pts = [
    { x: HARNESS.x + HARNESS.w, y: sy },
    { x: BUS_X, y: sy },
    { x: BUS_X, y: ty },
    { x: GATE.x, y: ty },
  ]
  if (verdict !== 'denied') pts.push({ x: ROW_X, y: ty })
  return pts
}

const toD = (pts: Point[]) => pts.map((p, i) => `${i ? 'L' : 'M'}${p.x} ${p.y}`).join(' ')

/**
 * AI agents graphic: a `harness::send` message starts an agent session. Its function calls pass
 * through an allow list (allowed, denied, or held for a human), the agent spawns sub-agents on their
 * own git worktrees, and every run lands as spans in one trace.
 */
export function AgentsHarness({ className }: { className?: string }) {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const [step, setStep] = useState(0)

  useEffect(() => {
    if (!active) return
    setStep(0)
    const id = window.setInterval(() => setStep((s) => s + 1), STEP_MS)
    return () => window.clearInterval(id)
  }, [active])

  const s = active ? step % SCRIPT.length : LAST
  const loop = active ? Math.floor(step / SCRIPT.length) : -1
  const current = SCRIPT[s]
  const call = active ? current.call : undefined
  const spawned = s >= 5
  const spanCount = SPANS.filter((sp) => sp.at <= s).length
  const agents = spawned ? 3 : 1
  const d = (delay: number) => (active ? delay : 0)

  return (
    <div ref={ref} className={cn('relative', className)}>
      <svg
        viewBox="0 0 400 470"
        role="img"
        aria-label="A harness::send message asking to refund order 1042 and email the customer starts an agent session allowed to call orders::* and email::send. The agent's call to orders::get is allowed, db::drop is denied, and orders::refund waits for a human to approve it. The agent spawns two sub-agents, each on its own git worktree: one calls email::send and is allowed, the other calls payments::payout and is denied. Every call from all three agents appears as a span in one trace."
        className="h-auto w-full overflow-visible"
      >
        {/* Message */}
        <rect
          x={MSG.x}
          y={MSG.y}
          width={MSG.w}
          height={MSG.h}
          rx={10}
          fill="var(--node)"
          stroke={active && current.send ? 'var(--hero-accent)' : 'var(--line-strong)'}
          style={{ transition: 'stroke 300ms ease' }}
        />
        <KindChip x={MSG.x + 12} y={MSG.y + 16} label="fn" />
        <text x={MSG.x + 40} y={MSG.y + 21} className="fill-foreground font-mono" fontSize={11}>
          harness::send
        </text>
        <text
          x={MSG.x + MSG.w - 12}
          y={MSG.y + 21}
          textAnchor="end"
          className="fill-muted-foreground font-mono"
          fontSize={9.5}
        >
          iii.trigger
        </text>
        <text x={MSG.x + 40} y={MSG.y + 38} className="fill-muted-foreground font-mono" fontSize={10.5}>
          “Refund order 1042 and email the customer”
        </text>
        <line x1={74} y1={MSG.y + MSG.h} x2={74} y2={HARNESS.y} stroke="var(--line-strong)" />

        {/* Allow list header over the gate */}
        <text x={GATE.x + 3} y={87} textAnchor="middle" className="fill-muted-foreground font-mono" fontSize={9.5}>
          allow
        </text>
        <Pattern x={ROW_X} w={68} label="orders::*" />
        <Pattern x={ROW_X + 76} w={80} label="email::send" />

        {/* Static wiring: sources → bus → gate → rows */}
        <line x1={BUS_X} y1={sourceCy.harness} x2={BUS_X} y2={rowCy('orders::refund')} stroke="var(--line)" />
        {(['harness', 'sub1', 'sub2'] as Source[]).map((src) => (
          <line
            key={src}
            x1={HARNESS.x + HARNESS.w}
            y1={sourceCy[src]}
            x2={BUS_X}
            y2={sourceCy[src]}
            stroke="var(--line)"
            strokeDasharray={src === 'harness' ? undefined : '3 4'}
          />
        ))}
        {ROWS.map((id) => (
          <g key={id}>
            <line x1={BUS_X} y1={rowCy(id)} x2={GATE.x} y2={rowCy(id)} stroke="var(--line)" />
            <line x1={GATE.x + GATE.w} y1={rowCy(id)} x2={ROW_X} y2={rowCy(id)} stroke="var(--line)" />
          </g>
        ))}
        <line
          x1={APPROVAL_X}
          y1={rowY('orders::refund') + ROW_H}
          x2={APPROVAL_X}
          y2={APPROVAL.y}
          stroke="var(--line-strong)"
          strokeDasharray="3 4"
        />
        <rect
          x={GATE.x}
          y={GATE.y}
          width={GATE.w}
          height={GATE.h}
          rx={3}
          fill="var(--faint)"
          stroke="var(--line-strong)"
        />

        {/* Harness */}
        <rect
          x={HARNESS.x}
          y={HARNESS.y}
          width={HARNESS.w}
          height={HARNESS.h}
          rx={10}
          fill="var(--node)"
          stroke={active && call?.from === 'harness' ? 'var(--hero-accent)' : 'var(--line-strong)'}
          style={{ transition: 'stroke 300ms ease' }}
        />
        <KindChip x={HARNESS.x + 8} y={HARNESS.y + 9} label="ai" />
        <text x={HARNESS.x + 34} y={HARNESS.y + 22} className="fill-foreground font-mono" fontSize={11}>
          harness
        </text>

        {/* Sub-agents on their own worktrees */}
        <motion.path
          d={`M28 ${HARNESS.y + HARNESS.h} V${sourceCy.sub2} H${SUBS.sub2.x} M28 ${sourceCy.sub1} H${SUBS.sub1.x}`}
          fill="none"
          stroke="var(--line-strong)"
          initial={false}
          animate={{ pathLength: spawned ? 1 : 0, opacity: spawned ? 1 : 0 }}
          transition={{ duration: 0.7, ease }}
        />
        {(['sub1', 'sub2'] as const).map((id, i) => {
          const b = SUBS[id]
          return (
            <motion.g
              key={id}
              initial={false}
              animate={{ opacity: spawned ? 1 : 0.3, x: spawned ? 0 : -4 }}
              transition={{ duration: 0.5, delay: spawned ? d(0.4 + i * 0.15) : 0, ease }}
            >
              <rect
                x={b.x}
                y={b.y}
                width={b.w}
                height={b.h}
                rx={10}
                fill="var(--node)"
                stroke={active && call?.from === id ? 'var(--hero-accent)' : 'var(--line-strong)'}
                strokeDasharray="3 3"
                style={{ transition: 'stroke 300ms ease' }}
              />
              <text x={b.x + 10} y={b.y + 17} className="fill-foreground font-mono" fontSize={11}>
                sub-agent
              </text>
              <BranchGlyph x={b.x + 10} y={b.y + 24} />
              <text x={b.x + 21} y={b.y + 32} className="fill-muted-foreground font-mono" fontSize={8.5}>
                {b.branch}
              </text>
            </motion.g>
          )
        })}
        <text x={12} y={296} className="fill-muted-foreground font-mono" fontSize={9.5}>
          max depth 2
        </text>
        <text x={12} y={310} className="fill-muted-foreground font-mono" fontSize={9.5}>
          max fan-out 4
        </text>

        {/* Function rows */}
        {ROWS.map((id) => {
          const status = rowStatus(id, s)
          const y = rowY(id)
          const targeted = active && call?.to === id
          return (
            <g key={id}>
              <rect
                x={ROW_X}
                y={y}
                width={ROW_W}
                height={ROW_H}
                rx={8}
                fill="var(--node)"
                stroke={targeted ? 'var(--hero-accent)' : 'var(--line-strong)'}
                style={{ transition: 'stroke 300ms ease' }}
              />
              <text
                x={ROW_X + 12}
                y={y + 19}
                className={status === 'denied' ? 'fill-muted-foreground font-mono' : 'fill-foreground font-mono'}
                fontSize={11}
              >
                {id}
              </text>
              <AnimatePresence mode="wait" initial={false}>
                <motion.g
                  key={`${status}-${loop}`}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0, transition: { duration: 0.2 } }}
                  transition={{
                    duration: 0.3,
                    delay: targeted ? d(TRAVEL) : id === 'orders::refund' && s === 4 ? d(0.7) : 0,
                  }}
                >
                  <Status x={ROW_X + ROW_W - 12} y={y + ROW_H / 2} status={status} active={active} held />
                </motion.g>
              </AnimatePresence>
            </g>
          )
        })}

        {/* Human approval */}
        <rect
          x={APPROVAL.x}
          y={APPROVAL.y}
          width={APPROVAL.w}
          height={APPROVAL.h}
          rx={10}
          fill="var(--node)"
          stroke={active && (s === 3 || s === 4) ? 'var(--hero-accent)' : 'var(--line-strong)'}
          style={{ transition: 'stroke 300ms ease' }}
        />
        <rect
          x={APPROVAL.x + 10}
          y={APPROVAL.y + 11}
          width={18}
          height={18}
          rx={5}
          fill="var(--faint)"
          stroke="var(--line)"
        />
        <circle cx={APPROVAL.x + 19} cy={APPROVAL.y + 17.5} r={2.6} fill="none" stroke="var(--muted-foreground)" />
        <path
          d={`M${APPROVAL.x + 14} ${APPROVAL.y + 25.5} a5 4.5 0 0 1 10 0`}
          fill="none"
          stroke="var(--muted-foreground)"
        />
        <text x={APPROVAL.x + 36} y={APPROVAL.y + 17} className="fill-foreground font-mono" fontSize={11}>
          human approval
        </text>
        <AnimatePresence mode="wait" initial={false}>
          <motion.g
            key={`approval-${s >= 4 ? 'done' : s === 3 ? 'wait' : 'idle'}-${loop}`}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, transition: { duration: 0.2 } }}
            transition={{ duration: 0.3, delay: s === 3 ? d(TRAVEL + 0.4) : s === 4 ? d(0.3) : 0 }}
          >
            <text x={APPROVAL.x + 36} y={APPROVAL.y + 31} className="fill-muted-foreground font-mono" fontSize={9.5}>
              {s >= 4 ? 'approved by a person' : s === 3 ? 'waiting on a person' : 'on sensitive calls'}
            </text>
            <Status
              x={APPROVAL.x + APPROVAL.w - 12}
              y={APPROVAL.y + APPROVAL.h / 2}
              status={s >= 4 ? 'approved' : s === 3 ? 'waiting' : 'idle'}
              active={active}
              bare
            />
          </motion.g>
        </AnimatePresence>

        {/* Active call */}
        <AnimatePresence>
          {active && current.send ? (
            <motion.g key={`send-${loop}`} exit={{ opacity: 0 }}>
              <ActivePath
                pts={[
                  { x: 74, y: MSG.y + MSG.h },
                  { x: 74, y: HARNESS.y },
                ]}
                duration={0.6}
              />
              <Packet
                pts={[
                  { x: 74, y: MSG.y + MSG.h },
                  { x: 74, y: HARNESS.y },
                ]}
                duration={0.6}
              />
            </motion.g>
          ) : null}
          {call ? (
            <motion.g key={`call-${s}-${loop}`} exit={{ opacity: 0 }} transition={{ duration: 0.4 }}>
              <ActivePath pts={callPoints(call.from, call.to, call.verdict)} duration={TRAVEL} />
              <Packet pts={callPoints(call.from, call.to, call.verdict)} duration={TRAVEL} />
              {call.verdict === 'denied' ? <DenyMark x={GATE.x + GATE.w / 2} y={rowCy(call.to)} /> : null}
              {call.verdict === 'waiting' ? (
                <>
                  <ActivePath
                    pts={[
                      { x: APPROVAL_X, y: rowY(call.to) + ROW_H },
                      { x: APPROVAL_X, y: APPROVAL.y },
                    ]}
                    duration={0.35}
                    delay={TRAVEL}
                  />
                  <Packet
                    pts={[
                      { x: APPROVAL_X, y: rowY(call.to) + ROW_H },
                      { x: APPROVAL_X, y: APPROVAL.y },
                    ]}
                    duration={0.35}
                    delay={TRAVEL}
                  />
                </>
              ) : null}
            </motion.g>
          ) : null}
          {active && current.approve ? (
            <motion.g key={`approve-${loop}`} exit={{ opacity: 0 }}>
              <ActivePath
                pts={[
                  { x: APPROVAL_X, y: APPROVAL.y },
                  { x: APPROVAL_X, y: rowY('orders::refund') + ROW_H },
                ]}
                duration={0.4}
                delay={0.3}
              />
              <Packet
                pts={[
                  { x: APPROVAL_X, y: APPROVAL.y },
                  { x: APPROVAL_X, y: rowY('orders::refund') + ROW_H },
                ]}
                duration={0.4}
                delay={0.3}
              />
            </motion.g>
          ) : null}
        </AnimatePresence>

        {/* One trace */}
        <line x1={12} y1={TRACE_Y} x2={388} y2={TRACE_Y} stroke="var(--line)" />
        <text x={12} y={TRACE_Y + 20} className="fill-muted-foreground font-mono" fontSize={9.5}>
          trace <tspan className="fill-foreground">4bf92f3c</tspan>
        </text>
        <text
          x={388}
          y={TRACE_Y + 20}
          textAnchor="end"
          className="fill-muted-foreground font-mono"
          fontSize={9.5}
          style={{ fontVariantNumeric: 'tabular-nums' }}
        >
          {spanCount} {spanCount === 1 ? 'span' : 'spans'} · {agents} {agents === 1 ? 'agent' : 'agents'}
        </text>
        <g key={`trace-${loop}`}>
          {SPANS.map((sp, i) => {
            if (sp.at > s) return null
            const end = Math.min(sp.end, s + 0.95)
            const waiting = sp.waitUntil !== undefined && s < sp.waitUntil
            const y = TRACE_Y + 30 + i * 8
            const fresh = active && sp.at === s
            return (
              // biome-ignore lint/suspicious/noArrayIndexKey: fixed, ordered span list
              <g key={i}>
                <motion.rect
                  x={tx(sp.start)}
                  y={y}
                  height={5}
                  rx={1.5}
                  initial={active ? { width: 0 } : false}
                  animate={{ width: tx(end) - tx(sp.start) }}
                  transition={{ duration: active ? 0.8 : 0, delay: fresh && sp.at > 0 ? d(0.5) : 0, ease }}
                  fill={fresh ? 'var(--foreground)' : 'var(--line-strong)'}
                  style={{ transition: 'fill 600ms ease' }}
                />
                {sp.fail || waiting ? (
                  <motion.circle
                    cx={tx(end) + 5}
                    cy={y + 2.5}
                    r={2}
                    fill={sp.fail ? 'var(--fail)' : 'var(--warn)'}
                    initial={active ? { opacity: 0 } : false}
                    animate={{ opacity: 1 }}
                    transition={{ duration: 0.3, delay: d(1.2) }}
                  />
                ) : null}
              </g>
            )
          })}
        </g>
      </svg>

      <div className="mt-3 flex h-10 items-center justify-center overflow-hidden px-2 text-center font-mono text-muted-foreground text-xs sm:h-6">
        <AnimatePresence mode="wait" initial={false}>
          <motion.span
            key={s}
            className="line-clamp-2 text-pretty sm:truncate"
            initial={{ opacity: 0, y: 6, filter: 'blur(2px)' }}
            animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
            exit={{ opacity: 0, y: -6, filter: 'blur(2px)' }}
            transition={{ duration: 0.3, ease }}
          >
            {current.caption}
          </motion.span>
        </AnimatePresence>
      </div>
    </div>
  )
}

function KindChip({ x, y, label }: { x: number; y: number; label: string }) {
  return (
    <>
      <rect x={x} y={y} width={18} height={18} rx={5} fill="var(--faint)" stroke="var(--line)" />
      <text x={x + 9} y={y + 12.2} textAnchor="middle" className="fill-muted-foreground font-mono" fontSize={8.5}>
        {label}
      </text>
    </>
  )
}

function Pattern({ x, w, label }: { x: number; w: number; label: string }) {
  return (
    <>
      <rect x={x} y={74} width={w} height={18} rx={5} fill="var(--faint)" stroke="var(--line-strong)" />
      <text x={x + w / 2} y={86.5} textAnchor="middle" className="fill-foreground font-mono" fontSize={10}>
        {label}
      </text>
    </>
  )
}

function BranchGlyph({ x, y }: { x: number; y: number }) {
  return (
    <g fill="none" stroke="var(--muted-foreground)" strokeWidth={0.9}>
      <circle cx={x + 1.5} cy={y + 1.5} r={1.4} />
      <circle cx={x + 1.5} cy={y + 8.5} r={1.4} />
      <circle cx={x + 6.5} cy={y + 3} r={1.4} />
      <path d={`M${x + 1.5} ${y + 3} V${y + 7} M${x + 6.5} ${y + 4.4} Q${x + 6.5} ${y + 6.5} ${x + 2.6} ${y + 7.4}`} />
    </g>
  )
}

type StatusKind = 'idle' | 'allowed' | 'denied' | 'waiting' | 'approved'

type StatusProps = { x: number; y: number; status: StatusKind; active: boolean; held?: boolean; bare?: boolean }

function Status({ x, y, status, active, held, bare }: StatusProps) {
  if (status === 'idle') {
    return <circle cx={x - 2} cy={y} r={2} fill="var(--line-strong)" />
  }
  const label = status === 'waiting' && held ? 'held' : status
  return (
    <g>
      {bare ? null : (
        <text x={x - 12} y={y + 3.3} textAnchor="end" className="fill-muted-foreground font-mono" fontSize={9.5}>
          {label}
        </text>
      )}
      {status === 'denied' ? (
        <path
          d={`M${x - 5} ${y - 3} l6 6 M${x + 1} ${y - 3} l-6 6`}
          stroke="var(--fail)"
          strokeWidth={1.4}
          strokeLinecap="round"
        />
      ) : status === 'waiting' ? (
        <motion.circle
          cx={x - 2}
          cy={y}
          r={2.6}
          fill="var(--warn)"
          animate={active ? { opacity: [1, 0.35, 1] } : { opacity: 1 }}
          transition={active ? { duration: 1.2, repeat: Number.POSITIVE_INFINITY, ease: easeInOut } : undefined}
        />
      ) : status === 'approved' ? (
        <path
          d={`M${x - 6} ${y} l2.6 2.6 l5 -5.2`}
          fill="none"
          stroke="var(--ok)"
          strokeWidth={1.5}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      ) : (
        <circle cx={x - 2} cy={y} r={2.6} fill="var(--ok)" />
      )}
    </g>
  )
}

function DenyMark({ x, y }: { x: number; y: number }) {
  return (
    <motion.g
      initial={{ opacity: 0, scale: 0.6 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.3, delay: TRAVEL, ease }}
      style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
    >
      <circle cx={x} cy={y} r={7} fill="var(--node)" stroke="var(--fail)" />
      <path
        d={`M${x - 2.6} ${y - 2.6} l5.2 5.2 M${x + 2.6} ${y - 2.6} l-5.2 5.2`}
        stroke="var(--fail)"
        strokeWidth={1.3}
        strokeLinecap="round"
      />
    </motion.g>
  )
}

function ActivePath({ pts, duration, delay = 0 }: { pts: Point[]; duration: number; delay?: number }) {
  return (
    <motion.path
      d={toD(pts)}
      fill="none"
      stroke="var(--hero-accent)"
      strokeWidth={1.25}
      initial={{ pathLength: 0, opacity: 0.9 }}
      animate={{ pathLength: 1, opacity: [0.9, 0.9, 0.3] }}
      transition={{
        pathLength: { duration, delay, ease: 'linear' },
        opacity: { duration: duration + 1.2, delay, times: [0, 0.6, 1] },
      }}
    />
  )
}

function Packet({ pts, duration, delay = 0 }: { pts: Point[]; duration: number; delay?: number }) {
  const lens = pts.slice(1).map((p, i) => Math.hypot(p.x - pts[i].x, p.y - pts[i].y))
  const total = lens.reduce((a, b) => a + b, 0) || 1
  const times = [0]
  for (const l of lens) times.push(times[times.length - 1] + l / total)
  return (
    <motion.circle
      r={3}
      fill="var(--foreground)"
      initial={{ cx: pts[0].x, cy: pts[0].y, opacity: 0 }}
      animate={{ cx: pts.map((p) => p.x), cy: pts.map((p) => p.y), opacity: [0, 1, 1, 0] }}
      transition={{
        cx: { duration, delay, times, ease: 'linear' },
        cy: { duration, delay, times, ease: 'linear' },
        opacity: { duration, delay, times: [0, 0.1, 0.9, 1] },
      }}
    />
  )
}
