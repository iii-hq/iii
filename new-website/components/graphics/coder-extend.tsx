'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { easeInOut, easeOut } from '@/lib/motion'

import { CoderFrame, ease, StatusDot, SvgLabel, useCoderSteps } from './coder-kit'

type Joiner = { id: string; chip: string; tab: string; say: string; call: string }

const EXISTING = ['orders-api', 'billing.py']
const JOINERS: Joiner[] = [
  { id: 'kanban', chip: 'kanban', tab: 'board', say: 'Opened the board', call: 'kanban::move' },
  { id: 'ios', chip: 'ios-sim', tab: 'iPhone', say: 'Booted a simulator', call: 'sim::tap' },
  { id: 'chromium', chip: 'chromium', tab: 'chromium', say: 'Driving a real tab', call: 'browser::click' },
]

const chipW = (label: string) => label.length * 6 + 16
const CHIP_LABELS = [...EXISTING, ...JOINERS.map((j) => j.chip)]
const CHIP_X = CHIP_LABELS.map((_, i) => CHIP_LABELS.slice(0, i).reduce((x, l) => x + chipW(l) + 6, 84))

const P = { x: 176, top: 106, tabH: 22, right: 444, bottom: 328 }
const TAB_W = 70
const tabX = (i: number) => P.x + 8 + i * (TAB_W + 4)
const START_UPTIME = 14 * 86400 + 3 * 3600 + 12 * 60 + 47

function formatUptime(s: number) {
  const d = Math.floor(s / 86400)
  const h = Math.floor((s % 86400) / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = s % 60
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d}d ${pad(h)}:${pad(m)}:${pad(sec)}`
}

/**
 * Extensibility: workers join a running system with no restart (the uptime keeps ticking)
 * and ship UI into the ADE: a board, an iPhone simulator and a Chromium tab open beside the chat.
 */
export function CoderExtend() {
  const { ref, active, step } = useCoderSteps(JOINERS.length, 3000)
  const joined = active ? step : JOINERS.length - 1
  const view = active ? step : 0
  const [uptime, setUptime] = useState(START_UPTIME)

  useEffect(() => {
    if (!active) return
    const id = window.setInterval(() => setUptime((u) => u + 1), 1000)
    return () => window.clearInterval(id)
  }, [active])

  const j = JOINERS[view]
  const caption = active
    ? `${JOINERS[step].chip} joined · no restart · ${JOINERS[step].tab} opened beside the chat`
    : 'three workers joined · 0 restarts · their pages open beside the chat'

  return (
    <CoderFrame
      frameRef={ref}
      label="A running iii system with a ticking uptime counter and zero restarts. Kanban, iOS simulator and Chromium workers join one after another, and each ships a page into the iii ADE that opens beside the chat: a kanban board, an iPhone simulator and a browser tab."
      caption={caption}
      captionKey={active ? step : 'static'}
    >
      {/* Engine status */}
      <rect x={16} y={12} width={428} height={28} rx={8} fill="var(--node)" stroke="var(--line-strong)" />
      <circle cx={30} cy={26} r={2.75} fill="var(--ok)" />
      <text x={40} y={29.5} className="fill-foreground font-mono" fontSize={10.5}>
        engine
      </text>
      <text x={96} y={29.5} className="fill-muted-foreground font-mono" fontSize={10}>
        uptime
      </text>
      <text x={140} y={29.5} className="fill-foreground font-mono tabular-nums" fontSize={10.5}>
        {formatUptime(uptime)}
      </text>
      <text x={432} y={29.5} textAnchor="end" className="fill-muted-foreground font-mono" fontSize={10}>
        restarts <tspan className="fill-foreground">0</tspan>
      </text>

      {/* Workers */}
      <SvgLabel x={16} y={67}>
        WORKERS
      </SvgLabel>
      {CHIP_LABELS.map((label, i) => {
        const isNew = i >= EXISTING.length
        const visible = !isNew || i - EXISTING.length <= joined
        const justJoined = active && isNew && i - EXISTING.length === step
        return (
          <motion.g
            key={label}
            initial={false}
            animate={{ opacity: visible ? 1 : 0, y: visible ? 0 : -8 }}
            transition={{ duration: 0.5, ease }}
          >
            <rect
              x={CHIP_X[i]}
              y={52}
              width={chipW(label)}
              height={20}
              rx={6}
              fill="var(--node)"
              stroke={justJoined ? 'var(--hero-accent)' : 'var(--line-strong)'}
              style={{ transition: 'stroke 400ms ease' }}
            />
            <text x={CHIP_X[i] + 8} y={65.5} className="fill-foreground font-mono" fontSize={10}>
              {label}
            </text>
          </motion.g>
        )
      })}

      {/* ADE window */}
      <rect x={16} y={84} width={428} height={244} rx={10} fill="var(--node)" stroke="var(--line-strong)" />
      <SvgLabel x={28} y={99}>
        III ADE
      </SvgLabel>
      <line x1={16} x2={444} y1={P.top} y2={P.top} stroke="var(--line)" />
      <line x1={P.x} x2={P.x} y1={P.top} y2={P.bottom} stroke="var(--line)" />

      {/* Chat */}
      <rect x={60} y={116} width={106} height={20} rx={6} fill="var(--faint)" stroke="var(--line)" />
      <text x={70} y={129.5} className="fill-foreground" fontSize={9.5}>
        Fix the flaky test
      </text>
      {JOINERS.map((m, i) => {
        const y = 150 + i * 56
        const shown = i <= joined
        return (
          <motion.g
            key={m.id}
            initial={false}
            animate={{ opacity: shown ? 1 : 0, y: shown ? 0 : 6 }}
            transition={{ duration: 0.45, delay: shown && active ? 0.7 : 0, ease }}
          >
            <text x={26} y={y + 4} className="fill-muted-foreground" fontSize={9.5}>
              {m.say}
            </text>
            <rect x={26} y={y + 12} width={134} height={26} rx={6} fill="var(--node)" stroke="var(--line-strong)" />
            <rect x={32} y={y + 18} width={14} height={14} rx={4} fill="var(--faint)" stroke="var(--line)" />
            <text x={39} y={y + 28} textAnchor="middle" className="fill-muted-foreground font-mono" fontSize={7}>
              fn
            </text>
            <text x={52} y={y + 28.5} className="fill-foreground font-mono" fontSize={9}>
              {m.call}
            </text>
            <circle cx={150} cy={y + 25} r={2.5} fill="var(--ok)" />
          </motion.g>
        )
      })}

      {/* Worker-shipped pages */}
      {JOINERS.map((t, i) => {
        const shown = i <= joined
        const selected = i === view
        const x = tabX(i)
        return (
          <motion.g
            key={t.id}
            initial={false}
            animate={{ opacity: shown ? 1 : 0 }}
            transition={{ duration: 0.4, delay: shown && active ? 0.5 : 0 }}
          >
            <text
              x={x + TAB_W / 2}
              y={P.top + 15}
              textAnchor="middle"
              className={selected ? 'fill-foreground font-mono' : 'fill-muted-foreground font-mono'}
              fontSize={9.5}
            >
              {t.tab}
            </text>
          </motion.g>
        )
      })}
      <line x1={P.x} x2={P.right} y1={P.top + P.tabH} y2={P.top + P.tabH} stroke="var(--line)" />
      <motion.line
        y1={P.top + P.tabH}
        y2={P.top + P.tabH}
        stroke="var(--hero-accent)"
        initial={false}
        animate={{ x1: tabX(view) + 8, x2: tabX(view) + TAB_W - 8 }}
        transition={{ duration: 0.45, delay: active ? 0.5 : 0, ease }}
      />

      <AnimatePresence mode="wait" initial={false}>
        <motion.g
          key={j.id}
          initial={{ opacity: 0, x: 16 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.5, delay: active ? 0.55 : 0, ease }}
        >
          {view === 0 ? (
            <Board active={active} />
          ) : view === 1 ? (
            <Phone active={active} />
          ) : (
            <Browser active={active} />
          )}
          <HotReload active={active} />
        </motion.g>
      </AnimatePresence>
    </CoderFrame>
  )
}

function HotReload({ active }: { active: boolean }) {
  return (
    <g>
      {active ? <StatusDot cx={P.right - 78} cy={P.top + 11} delay={0.9} r={2.25} /> : null}
      <text
        x={P.right - 10}
        y={P.top + 14.5}
        textAnchor="end"
        className="fill-muted-foreground font-mono"
        fontSize={8.5}
      >
        hot-reloaded
      </text>
    </g>
  )
}

const COLS = ['todo', 'doing', 'done']
const colX = (i: number) => P.x + 12 + i * 86

function Board({ active }: { active: boolean }) {
  return (
    <g>
      {COLS.map((c, i) => (
        <g key={c}>
          <rect x={colX(i)} y={140} width={78} height={176} rx={7} fill="var(--faint)" stroke="var(--line)" />
          <text x={colX(i) + 8} y={155} className="fill-muted-foreground font-mono" fontSize={8.5}>
            {c}
          </text>
        </g>
      ))}
      <Card x={colX(0) + 6} y={164} label="#44 docs" />
      <Card x={colX(0) + 6} y={192} label="#45 i18n" />
      <Card x={colX(2) + 6} y={164} label="#40 auth" />
      <motion.g
        initial={active ? { x: 0, y: 0 } : false}
        animate={{ x: colX(2) - colX(1), y: 28 }}
        transition={{ duration: 0.9, delay: 1.5, ease: easeInOut }}
      >
        <Card x={colX(1) + 6} y={164} label="#42 flaky" strong />
        <text x={colX(1) + 10} y={200} className="fill-muted-foreground font-mono" fontSize={7.5}>
          agent
        </text>
      </motion.g>
    </g>
  )
}

function Card({ x, y, label, strong }: { x: number; y: number; label: string; strong?: boolean }) {
  return (
    <g>
      <rect
        x={x}
        y={y}
        width={66}
        height={22}
        rx={5}
        fill="var(--node)"
        stroke={strong ? 'var(--hero-accent)' : 'var(--line-strong)'}
      />
      <text x={x + 7} y={y + 14.5} className="fill-foreground font-mono" fontSize={8.5}>
        {label}
      </text>
    </g>
  )
}

function Phone({ active }: { active: boolean }) {
  const cx = (P.x + P.right) / 2
  const w = 92
  const top = 138
  const h = 182
  return (
    <g>
      <rect x={cx - w / 2} y={top} width={w} height={h} rx={16} fill="var(--node)" stroke="var(--line-strong)" />
      <rect x={cx - 14} y={top + 7} width={28} height={7} rx={3.5} fill="var(--line-strong)" />
      <text x={cx - w / 2 + 12} y={top + 32} className="fill-foreground" fontSize={9}>
        Sign in
      </text>
      <rect
        x={cx - w / 2 + 10}
        y={top + 42}
        width={w - 20}
        height={16}
        rx={4}
        fill="var(--faint)"
        stroke="var(--line)"
      />
      <rect
        x={cx - w / 2 + 10}
        y={top + 64}
        width={w - 20}
        height={16}
        rx={4}
        fill="var(--faint)"
        stroke="var(--line)"
      />
      <rect x={cx - w / 2 + 10} y={top + 90} width={w - 20} height={18} rx={5} fill="var(--foreground)" />
      <text x={cx} y={top + 102} textAnchor="middle" className="fill-background" fontSize={8.5}>
        Continue
      </text>
      <rect x={cx - 16} y={top + h - 10} width={32} height={3} rx={1.5} fill="var(--line-strong)" />
      {active ? (
        <motion.circle
          cx={cx + 8}
          cy={top + 99}
          r={6}
          fill="none"
          stroke="var(--hero-accent)"
          style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
          initial={{ scale: 0.3, opacity: 0 }}
          animate={{ scale: [0.3, 2.2], opacity: [0, 0.8, 0] }}
          transition={{ duration: 0.9, delay: 1.6, ease: easeOut, repeat: 1, repeatDelay: 0.3 }}
        />
      ) : null}
      <text x={cx + w / 2 + 10} y={top + 102} className="fill-muted-foreground font-mono" fontSize={8.5}>
        ← you tap
      </text>
    </g>
  )
}

function Browser({ active }: { active: boolean }) {
  const x = P.x + 12
  const w = P.right - 12 - x
  const top = 138
  const bx = x + 16
  const by = top + 128
  return (
    <g>
      <rect x={x} y={top} width={w} height={180} rx={8} fill="var(--node)" stroke="var(--line-strong)" />
      <line x1={x} x2={x + w} y1={top + 22} y2={top + 22} stroke="var(--line)" />
      <rect x={x + 10} y={top + 6} width={w - 20} height={11} rx={5.5} fill="var(--faint)" stroke="var(--line)" />
      <text x={x + 18} y={top + 14.5} className="fill-muted-foreground font-mono" fontSize={7.5}>
        localhost:3000/checkout
      </text>
      <rect x={x + 16} y={top + 36} width={96} height={9} rx={3} fill="var(--line-strong)" />
      <rect x={x + 16} y={top + 54} width={w - 32} height={6} rx={3} fill="var(--line)" />
      <rect x={x + 16} y={top + 66} width={w - 72} height={6} rx={3} fill="var(--line)" />
      <rect x={x + 16} y={top + 88} width={w - 32} height={22} rx={5} fill="var(--faint)" stroke="var(--line)" />
      <rect x={bx} y={by} width={84} height={22} rx={5} fill="var(--foreground)" />
      <text x={bx + 42} y={by + 14.5} textAnchor="middle" className="fill-background" fontSize={9}>
        Place order
      </text>
      <motion.g
        initial={active ? { x: 150, y: -70 } : false}
        animate={{ x: 0, y: 0 }}
        transition={{ duration: 1.1, delay: 1.2, ease: easeInOut }}
      >
        <path
          d={`M ${bx + 60} ${by + 10} l 0 13 l 3.5 -3.2 l 2.6 5.6 l 2.2 -1 l -2.6 -5.6 l 4.8 -0.2 z`}
          fill="var(--foreground)"
          stroke="var(--node)"
          strokeWidth={1}
        />
        <rect x={bx + 72} y={by + 24} width={36} height={13} rx={4} fill="var(--foreground)" />
        <text x={bx + 90} y={by + 33.5} textAnchor="middle" className="fill-background font-mono" fontSize={7.5}>
          agent
        </text>
      </motion.g>
      {active ? (
        <motion.circle
          cx={bx + 60}
          cy={by + 11}
          r={6}
          fill="none"
          stroke="var(--hero-accent)"
          style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
          initial={{ scale: 0.3, opacity: 0 }}
          animate={{ scale: [0.3, 2.2], opacity: [0, 0.8, 0] }}
          transition={{ duration: 0.8, delay: 2.35, ease: easeOut }}
        />
      ) : null}
    </g>
  )
}
