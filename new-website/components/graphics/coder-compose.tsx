'use client'

import { AnimatePresence, motion } from 'motion/react'

import { CoderFrame, ease, KindChip, Packet, RouteLine, StatusDot, SvgLabel, useCoderSteps } from './coder-kit'

type Fn = { id: string; trigger?: boolean }
type Worker = { id: string; label: string; kind: string; version: string; fns: [Fn, Fn] }
type Caller = { id: string; label: string; kind: string; y: number }

const BASE: Worker = {
  id: 'orders',
  label: 'orders-api',
  kind: 'ts',
  version: '2.3.0',
  fns: [{ id: 'orders::create' }, { id: 'orders::refund' }],
}

const ADDS: (Worker & { caller: string; call: 0 | 1 })[] = [
  {
    id: 'kanban',
    label: 'kanban',
    kind: 'ts',
    version: '1.4.2',
    fns: [{ id: 'kanban::claim' }, { id: 'kanban::move' }],
    caller: 'agent',
    call: 0,
  },
  {
    id: 'worktree',
    label: 'worktrees',
    kind: 'rs',
    version: '0.9.1',
    fns: [{ id: 'worktree::create' }, { id: 'worktree::land' }],
    caller: 'ci',
    call: 1,
  },
  {
    id: 'ios',
    label: 'ios-sim-farm',
    kind: 'sw',
    version: '2.1.0',
    fns: [{ id: 'sim::boot' }, { id: 'sim::tap' }],
    caller: 'agent',
    call: 1,
  },
  {
    id: 'pg',
    label: 'postgres',
    kind: 'rs',
    version: '3.0.4',
    fns: [{ id: 'pg::query' }, { id: 'pg::row-changed', trigger: true }],
    caller: 'billing',
    call: 0,
  },
]

const CALLERS: Caller[] = [
  { id: 'agent', label: 'claude-code', kind: 'ai', y: 150 },
  { id: 'ci', label: 'ci-runner', kind: 'ci', y: 212 },
  { id: 'billing', label: 'billing.py', kind: 'py', y: 274 },
]
const callerById = Object.fromEntries(CALLERS.map((c) => [c.id, c])) as Record<string, Caller>

const W = { x: 16, w: 132, h: 36 }
const R = { x: 172, w: 148, top: 52, bottom: 316 }
const K = { x: 340, w: 104, h: 32 }

const rowY = (i: number) => 96 + i * 21
const workerY = (slot: number) => rowY(slot * 2) + 10.5

const STEP_MS = 1125

/**
 * Composability: workers drop into a running system one after another; their functions
 * snap into the shared registry and other workers, the agent included, call them at once.
 */
export function CoderCompose() {
  const { ref, active, step } = useCoderSteps(ADDS.length * 2, STEP_MS)
  const current = Math.floor(step / 2)
  const calling = step % 2 === 1
  const add = ADDS[current]
  const caller = callerById[add.caller]
  const callFn = add.fns[add.call].id
  const rowsVisible = 2 + (current + 1) * 2

  const caption = calling
    ? `${caller.label} → ${callFn} → ${add.label}`
    : `${add.label}@${add.version} running · ${add.fns[0].id}, ${add.fns[1].id}`

  const targetRow = 2 + current * 2 + add.call
  const ty = rowY(targetRow)
  const wy = workerY(current + 1)

  return (
    <CoderFrame
      frameRef={ref}
      label="A kanban board, a git-worktree manager, an iOS simulator farm and a Postgres client join a running system one by one. Each registers its functions in the shared registry, and other workers, including an agent, call them immediately."
      caption={caption}
      captionKey={step}
    >
      {/* compose::add command */}
      <rect x={16} y={12} width={428} height={28} rx={8} fill="var(--node)" stroke="var(--line-strong)" />
      <text x={28} y={30} className="fill-muted-foreground font-mono" fontSize={10.5}>
        ›
      </text>
      <text x={40} y={30} className="fill-foreground font-mono" fontSize={10.5}>
        compose::add
      </text>
      <AnimatePresence mode="wait" initial={false}>
        <motion.text
          key={add.id}
          x={128}
          y={30}
          className="font-mono"
          fontSize={10.5}
          initial={{ opacity: 0, x: -6 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.3, ease }}
        >
          <tspan className="fill-foreground">{add.label}</tspan>
          <tspan className="fill-muted-foreground">@{add.version}</tspan>
        </motion.text>
      </AnimatePresence>
      <g key={`status-${add.id}`}>
        <StatusDot cx={386} cy={26.5} delay={0.45} />
        <motion.text
          x={394}
          y={30}
          className="fill-muted-foreground font-mono"
          fontSize={9.5}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.3, delay: 0.45 }}
        >
          running
        </motion.text>
      </g>

      {/* Registry */}
      <rect
        x={R.x}
        y={R.top}
        width={R.w}
        height={R.bottom - R.top}
        rx={10}
        fill="var(--node)"
        stroke="var(--line-strong)"
      />
      <SvgLabel x={R.x + 12} y={70}>
        REGISTRY
      </SvgLabel>
      <text x={R.x + R.w - 12} y={70} textAnchor="end" className="fill-muted-foreground font-mono" fontSize={9}>
        {rowsVisible} fns
      </text>
      <line x1={R.x} x2={R.x + R.w} y1={80} y2={80} stroke="var(--line)" />

      {[BASE, ...ADDS].flatMap((w, wi) =>
        w.fns.map((fn, fi) => {
          const i = wi * 2 + fi
          const visible = wi === 0 || wi - 1 <= current
          const isTarget = active && calling && i === targetRow
          return (
            <motion.g
              key={fn.id}
              initial={false}
              animate={{ opacity: visible ? 1 : 0, x: visible ? 0 : -8 }}
              transition={{ duration: 0.45, delay: visible && wi > 0 ? 0.35 + fi * 0.12 : 0, ease }}
            >
              <rect
                x={R.x + 6}
                y={rowY(i) - 9}
                width={R.w - 12}
                height={18}
                rx={5}
                fill={isTarget ? 'var(--faint)' : 'transparent'}
                stroke={isTarget ? 'var(--hero-accent)' : 'transparent'}
                style={{ transition: 'stroke 300ms ease, fill 300ms ease' }}
              />
              <text
                x={R.x + 14}
                y={rowY(i) + 3.5}
                className={wi === 0 ? 'fill-muted-foreground font-mono' : 'fill-foreground font-mono'}
                fontSize={10}
              >
                {fn.id}
              </text>
              {fn.trigger ? (
                <text
                  x={R.x + R.w - 12}
                  y={rowY(i) + 3}
                  textAnchor="end"
                  className="fill-muted-foreground font-mono"
                  fontSize={8}
                >
                  trigger
                </text>
              ) : null}
            </motion.g>
          )
        }),
      )}

      {/* Workers (left) */}
      {[BASE, ...ADDS].map((w, slot) => {
        const visible = slot === 0 || slot - 1 <= current
        const cy = workerY(slot)
        const isTarget = active && calling && slot === current + 1
        return (
          <motion.g
            key={w.id}
            initial={false}
            animate={{ opacity: visible ? 1 : 0, x: visible ? 0 : -14 }}
            transition={{ duration: 0.5, ease }}
          >
            <motion.line
              x1={W.x + W.w}
              x2={R.x}
              y1={cy}
              y2={cy}
              stroke="var(--line-strong)"
              initial={false}
              animate={{ pathLength: visible ? 1 : 0 }}
              transition={{ duration: 0.4, delay: visible ? 0.2 : 0 }}
            />
            <rect
              x={W.x}
              y={cy - W.h / 2}
              width={W.w}
              height={W.h}
              rx={10}
              fill="var(--node)"
              stroke={isTarget ? 'var(--hero-accent)' : 'var(--line-strong)'}
              style={{ transition: 'stroke 300ms ease' }}
            />
            <KindChip x={W.x + 8} y={cy - 9} kind={w.kind} />
            <text x={W.x + 34} y={cy - 1} className="fill-foreground font-mono" fontSize={10.5}>
              {w.label}
            </text>
            <text x={W.x + 34} y={cy + 11} className="fill-muted-foreground font-mono" fontSize={9}>
              @{w.version}
            </text>
          </motion.g>
        )
      })}

      {/* Callers (right) */}
      <SvgLabel x={K.x + K.w} y={70} anchor="end">
        CALLERS
      </SvgLabel>
      {CALLERS.map((c) => {
        const isSource = active && calling && c.id === add.caller
        return (
          <g key={c.id}>
            <line x1={R.x + R.w} x2={K.x} y1={c.y} y2={c.y} stroke="var(--line)" strokeDasharray="3 4" />
            <rect
              x={K.x}
              y={c.y - K.h / 2}
              width={K.w}
              height={K.h}
              rx={10}
              fill="var(--node)"
              stroke={isSource ? 'var(--hero-accent)' : 'var(--line-strong)'}
              style={{ transition: 'stroke 300ms ease' }}
            />
            <KindChip x={K.x + 7} y={c.y - 9} kind={c.kind} />
            <text x={K.x + 31} y={c.y + 3.5} className="fill-foreground font-mono" fontSize={10}>
              {c.label}
            </text>
          </g>
        )
      })}

      {/* Call route: caller → registry row → worker */}
      <AnimatePresence>
        {active && calling ? (
          <motion.g key={`call-${step}`} exit={{ opacity: 0 }} transition={{ duration: 0.3 }}>
            <RouteLine from={{ x: K.x, y: caller.y }} to={{ x: R.x + R.w - 6, y: ty }} duration={0.45} />
            <RouteLine from={{ x: R.x + 6, y: ty }} to={{ x: W.x + W.w, y: wy }} delay={0.5} duration={0.35} />
            <Packet from={{ x: K.x, y: caller.y }} to={{ x: R.x + R.w - 6, y: ty }} duration={0.45} />
            <Packet from={{ x: R.x + 6, y: ty }} to={{ x: W.x + W.w, y: wy }} delay={0.5} duration={0.35} />
            <StatusDot cx={W.x + W.w - 10} cy={wy - 10} delay={0.9} />
          </motion.g>
        ) : null}
      </AnimatePresence>
    </CoderFrame>
  )
}
