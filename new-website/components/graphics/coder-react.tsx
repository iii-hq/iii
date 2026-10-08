'use client'

import { AnimatePresence, motion } from 'motion/react'

import { easeInOut, easeOut } from '@/lib/motion'

import { CoderFrame, ease, KindChip, StatusDot, SvgLabel, useCoderSteps } from './coder-kit'

type Source = { id: string; label: string; y: number }

const BUILT_IN = ['http', 'cron', 'queue', 'pub/sub', 'stream']
const WORKER_DEFINED = [
  'state key changed',
  'postgres row changed',
  'file in storage',
  'worktree landed',
  'ticket comment',
  'agent turn done',
]

const SOURCES: Source[] = [
  ...BUILT_IN.map((label, i) => ({ id: label, label, y: 44 + i * 21 })),
  ...WORKER_DEFINED.map((label, i) => ({ id: label, label, y: 170 + i * 21 })),
]
const sourceById = Object.fromEntries(SOURCES.map((s) => [s.id, s])) as Record<string, Source>

const FX = 272
const FW = 172
const AGENT = { y: 234, h: 82 }

type Binding = { source: string; fn: string; y: number; condition?: string }
const BINDINGS: Binding[] = [
  { source: 'http', fn: 'orders::create', y: 44 },
  { source: 'cron', fn: 'reports::daily', y: 82 },
  { source: 'postgres row changed', fn: 'search::index', y: 120 },
  { source: 'file in storage', fn: 'images::resize', y: 158 },
  { source: 'worktree landed', fn: 'ci::verify', y: 196 },
  { source: 'ticket comment', fn: 'agent', y: AGENT.y + 22, condition: 'ticket == #42' },
]

const PORT_X = 166
const bindingPath = (b: Binding) => {
  const sy = sourceById[b.source].y
  return `M ${PORT_X} ${sy} C ${PORT_X + 56} ${sy}, ${FX - 56} ${b.y}, ${FX} ${b.y}`
}

const CAPTIONS = [
  'POST /orders → orders::create',
  'cron 0 0 9 * * * * → reports::daily',
  'postgres row changed → search::index',
  'file lands in storage → images::resize',
  'worktree landed → ci::verify',
  'ticket #42 gets a reply → the waiting agent wakes, no polling',
]

/**
 * Reactivity: built-in and worker-defined trigger sources fire and wake the functions bound
 * to them; a sleeping agent bound to a ticket wakes when the reply arrives instead of polling.
 */
export function CoderReact() {
  const { ref, active, step } = useCoderSteps(BINDINGS.length, 1500)
  const firing = BINDINGS[step]
  const agentAwake = step === BINDINGS.length - 1

  return (
    <CoderFrame
      frameRef={ref}
      label="Trigger sources, built in (HTTP, cron, queue, pub/sub, stream) and defined by workers (state key, Postgres row, file in storage, worktree landed, ticket comment, agent turn done), fire and wake the functions bound to them. An agent bound to a ticket comment sleeps until the reply arrives instead of running a polling loop."
      caption={active ? CAPTIONS[step] : 'any change can start work · the agent waits without polling'}
      captionKey={active ? step : 'static'}
    >
      <SvgLabel x={16} y={24}>
        TRIGGERS
      </SvgLabel>
      <SvgLabel x={16} y={152}>
        DEFINED BY WORKERS
      </SvgLabel>
      <line x1={16} x2={PORT_X} y1={140} y2={140} stroke="var(--line)" strokeDasharray="3 4" />
      <SvgLabel x={FX + FW} y={24} anchor="end">
        BOUND FUNCTIONS
      </SvgLabel>

      {/* Bindings */}
      {BINDINGS.map((b) => (
        <path key={b.fn} d={bindingPath(b)} fill="none" stroke="var(--line-strong)" strokeOpacity={0.6} />
      ))}
      <AnimatePresence>
        {active ? (
          <motion.path
            key={`fire-${step}`}
            d={bindingPath(firing)}
            fill="none"
            stroke="var(--hero-accent)"
            strokeWidth={1.25}
            initial={{ pathLength: 0, opacity: 1 }}
            animate={{ pathLength: 1, opacity: [1, 1, 0.35] }}
            exit={{ opacity: 0 }}
            transition={{
              pathLength: { duration: 0.6, delay: 0.25, ease: easeInOut },
              opacity: { duration: 1.2, delay: 0.25, times: [0, 0.7, 1] },
            }}
          />
        ) : null}
      </AnimatePresence>

      {/* Condition on the agent's binding */}
      {(() => {
        const b = BINDINGS[BINDINGS.length - 1]
        const sy = sourceById[b.source].y
        const mx = (PORT_X + FX) / 2
        const my = (sy + b.y) / 2
        return (
          <g>
            <rect x={mx - 38} y={my - 9} width={76} height={18} rx={5} fill="var(--node)" stroke="var(--line-strong)" />
            <text x={mx} y={my + 3} textAnchor="middle" className="fill-muted-foreground font-mono" fontSize={8.5}>
              {b.condition}
            </text>
          </g>
        )
      })()}

      {/* Sources */}
      {SOURCES.map((s) => {
        const isFiring = active && firing.source === s.id
        const bound = BINDINGS.some((b) => b.source === s.id)
        return (
          <g key={s.id}>
            {isFiring ? (
              <g key={`ripple-${step}`}>
                {[0, 0.25].map((d) => (
                  <motion.circle
                    key={d}
                    cx={22}
                    cy={s.y}
                    r={3}
                    fill="none"
                    stroke="var(--hero-accent)"
                    style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
                    initial={{ scale: 1, opacity: 0.8 }}
                    animate={{ scale: 5, opacity: 0 }}
                    transition={{ duration: 1, delay: d, ease: easeOut }}
                  />
                ))}
              </g>
            ) : null}
            <circle
              cx={22}
              cy={s.y}
              r={3}
              fill={isFiring ? 'var(--foreground)' : 'var(--node)'}
              stroke={bound ? 'var(--hero-accent)' : 'var(--line-strong)'}
              style={{ transition: 'fill 300ms ease' }}
            />
            <text
              x={34}
              y={s.y + 3.5}
              className={isFiring || bound ? 'fill-foreground font-mono' : 'fill-muted-foreground font-mono'}
              fontSize={10}
            >
              {s.label}
            </text>
            {bound ? <circle cx={PORT_X} cy={s.y} r={1.75} fill="var(--line-strong)" /> : null}
          </g>
        )
      })}

      {/* Functions */}
      {BINDINGS.slice(0, -1).map((b) => {
        const woke = active && firing.fn === b.fn
        return (
          <g key={b.fn}>
            <rect
              x={FX}
              y={b.y - 13}
              width={FW}
              height={26}
              rx={8}
              fill="var(--node)"
              stroke={woke ? 'var(--hero-accent)' : 'var(--line-strong)'}
              style={{ transition: 'stroke 300ms ease 500ms' }}
            />
            <text x={FX + 12} y={b.y + 3.5} className="fill-foreground font-mono" fontSize={10}>
              {b.fn}
            </text>
            {woke ? <StatusDot key={step} cx={FX + FW - 12} cy={b.y} delay={0.85} /> : null}
          </g>
        )
      })}

      {/* Sleeping agent */}
      <rect
        x={FX}
        y={AGENT.y}
        width={FW}
        height={AGENT.h}
        rx={10}
        fill="var(--node)"
        stroke={agentAwake ? 'var(--hero-accent)' : 'var(--line-strong)'}
        strokeDasharray={agentAwake ? undefined : '3 3'}
        style={{ transition: 'stroke 300ms ease 600ms' }}
      />
      <KindChip x={FX + 10} y={AGENT.y + 13} kind="ai" />
      <text x={FX + 36} y={AGENT.y + 25.5} className="fill-foreground font-mono" fontSize={10.5}>
        triage-agent
      </text>
      <AnimatePresence mode="wait" initial={false}>
        <motion.g
          key={agentAwake ? 'awake' : 'asleep'}
          initial={{ opacity: 0, y: 3 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -3 }}
          transition={{ duration: 0.35, delay: agentAwake && active ? 0.7 : 0, ease }}
        >
          <circle cx={FX + 14} cy={AGENT.y + 45} r={2.75} fill={agentAwake ? 'var(--ok)' : 'var(--line-strong)'} />
          <text x={FX + 24} y={AGENT.y + 48.5} className="fill-muted-foreground font-mono" fontSize={9.5}>
            {agentAwake ? 'awake · replying on #42' : 'asleep · waiting on #42'}
          </text>
        </motion.g>
      </AnimatePresence>
      <line x1={FX} x2={FX + FW} y1={AGENT.y + 58} y2={AGENT.y + 58} stroke="var(--line)" />
      <text x={FX + 12} y={AGENT.y + 73} className="fill-muted-foreground font-mono" fontSize={9}>
        {'while (!reply) poll(5s)'}
      </text>
      <line
        x1={FX + 10}
        x2={FX + 138}
        y1={AGENT.y + 70}
        y2={AGENT.y + 70}
        stroke="var(--muted-foreground)"
        strokeWidth={1}
      />
    </CoderFrame>
  )
}
