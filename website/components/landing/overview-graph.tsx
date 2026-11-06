'use client'

import { motion } from 'motion/react'
import { useEffect, useState } from 'react'

import { cn } from '@/lib/utils'

import { EngineHub, Flow, Runner, STROKE, T, TYPE, Wire } from './graph/kit'
import type { Point } from './graph/model'

/**
 * The Overview graph as a system map (2026-10-08, Anthony and Mike): every participant sits on one ring, labelled
 * with its name and where it runs (language · cloud), with the iii engine at the centre. The ring fills as workers
 * register, then one request is traced through the system: each call travels caller → engine → callee and leaves a
 * fading trail. Names, functions and languages are real registry workers (iii-hq/workers); the clouds are a sample
 * deployment spread across AWS, GCP and Azure.
 */

const VIEW = { y: 18, w: 900, h: 548 } as const
/** The ring: an ellipse a little wider than tall, so side labels have room and the drawing stays short. */
const RING = { cx: 450, cy: 292, rx: 248, ry: 206 } as const
const ENGINE_BOX = { x: RING.cx, y: RING.cy, w: 220, h: 64 } as const
/** Where the engine sits, as a share of the drawn height (the Overview lines it up with the middle of the copy). */
export const AXIS_RATIO = (RING.cy - VIEW.y) / VIEW.h

type Cloud = 'AWS' | 'GCP' | 'Azure'
type Participant = Point & {
  id: string
  label: string
  place: string
  fn?: string
  side: 'top' | 'right' | 'bottom' | 'left'
}
type Worker = { id: string; fn: string; lang: string; cloud: Cloud }

/** Seven registry workers in the order they join, each tagged with its language and where it runs. */
export const WORKERS: Worker[] = [
  { id: 'http', fn: 'http::on-request', lang: 'Rust', cloud: 'AWS' },
  { id: 'database', fn: 'database::execute', lang: 'Rust', cloud: 'GCP' },
  { id: 'harness', fn: 'agent::events', lang: 'Rust', cloud: 'AWS' },
  { id: 'llm-router', fn: 'router::chat', lang: 'Rust', cloud: 'Azure' },
  { id: 'provider-anthropic', fn: 'provider::anthropic::stream', lang: 'Rust', cloud: 'GCP' },
  { id: 'claude-code', fn: 'claude::run', lang: 'TypeScript', cloud: 'Azure' },
  { id: 'hermes', fn: 'hermes::send', lang: 'Python', cloud: 'AWS' },
]
/** The card's tag: language, then where it is deployed. */
const placeOf = (w: Worker) => `${w.lang} · ${w.cloud}`

/** Clockwise from the top. The request enters on the left; harness, which makes every call, sits at the top. */
const RING_ORDER = [
  'harness',
  'llm-router',
  'provider-anthropic',
  'database',
  'hermes',
  'claude-code',
  'request',
  'http',
]

const sideFor = (angle: number): Participant['side'] => {
  const c = Math.cos(angle)
  if (c > 0.3) return 'right'
  if (c < -0.3) return 'left'
  return Math.sin(angle) < 0 ? 'top' : 'bottom'
}

const PARTICIPANTS: Participant[] = RING_ORDER.map((id, i) => {
  const angle = -Math.PI / 2 + (i * 2 * Math.PI) / RING_ORDER.length
  const point = { x: RING.cx + RING.rx * Math.cos(angle), y: RING.cy + RING.ry * Math.sin(angle) }
  if (id === 'request') return { id, label: 'Request', place: 'incoming call', side: sideFor(angle), ...point }
  const w = WORKERS.find((worker) => worker.id === id) as Worker
  return { id, label: id, place: placeOf(w), fn: w.fn, side: sideFor(angle), ...point }
})
const byId = Object.fromEntries(PARTICIPANTS.map((p) => [p.id, p])) as Record<string, Participant>

type Beat =
  | { kind: 'engine'; caption: string }
  | { kind: 'join'; ids: string[]; caption: string }
  | { kind: 'call'; from: string; to: string; caption: string }
  | { kind: 'done'; caption: string }

/** The script: the engine alone, workers build up on the right, then a request is routed through the engine. */
export const SCRIPT: Beat[] = [
  { kind: 'engine', caption: 'one iii engine · nothing attached yet' },
  { kind: 'join', ids: ['http'], caption: 'http joins and registers its functions with the engine' },
  { kind: 'join', ids: ['database'], caption: 'database joins · the engine now knows both' },
  {
    kind: 'join',
    ids: ['harness', 'llm-router', 'provider-anthropic'],
    caption: 'harness joins · its dependencies llm-router and provider-anthropic come with it',
  },
  {
    kind: 'join',
    ids: ['claude-code', 'hermes'],
    caption: 'claude-code (TypeScript) and hermes (Python) join · seven workers, one engine',
  },
  { kind: 'call', from: 'request', to: 'harness', caption: 'a request arrives → iii engine → harness' },
  { kind: 'call', from: 'harness', to: 'llm-router', caption: 'harness → iii engine → llm-router (router::chat)' },
  {
    kind: 'call',
    from: 'llm-router',
    to: 'provider-anthropic',
    caption: 'llm-router → iii engine → provider-anthropic · dependencies route through the engine too',
  },
  { kind: 'call', from: 'harness', to: 'database', caption: 'harness → iii engine → database (database::execute)' },
  { kind: 'call', from: 'harness', to: 'claude-code', caption: 'harness → iii engine → claude-code (claude::run)' },
  { kind: 'call', from: 'harness', to: 'hermes', caption: 'harness → iii engine → hermes (hermes::send)' },
  {
    kind: 'call',
    from: 'harness',
    to: 'request',
    caption: 'the response goes back the same way: harness → iii engine → caller',
  },
  { kind: 'done', caption: 'every call went through the engine · no worker talked to another directly' },
]
/** How long each kind of beat holds. Setup beats are quick, a call gets room for its reply to land, and the
    finished graph rests longest so the whole system can be read before the loop starts over. */
const BEAT_MS: Record<Beat['kind'], number> = { engine: 1000, join: 1100, call: 1300, done: 2400 }
export const captionFor = (step: number) => SCRIPT[Math.min(step, SCRIPT.length - 1)].caption

/** Counts beats on a loop while active; rests on the last beat (the complete graph) when idle. */
export function useBeat(active: boolean) {
  const [tick, setTick] = useState(0)
  useEffect(() => {
    setTick(0)
    if (!active) return
    let current = 0
    let id = 0
    const schedule = () => {
      id = window.setTimeout(
        () => {
          current += 1
          setTick(current)
          schedule()
        },
        BEAT_MS[SCRIPT[current % SCRIPT.length].kind],
      )
    }
    schedule()
    return () => window.clearTimeout(id)
  }, [active])
  return { step: active ? tick % SCRIPT.length : SCRIPT.length - 1, cycle: Math.floor(tick / SCRIPT.length) }
}

/* ---------- Geometry: every call is caller → engine centre → callee, along the spokes. ---------- */

const centre: Point = { x: RING.cx, y: RING.cy }
/** The path a packet follows for one call. */
const route = (from: string, to: string): Point[] => [byId[from], centre, byId[to]]

const joinedAt = (id: string) => SCRIPT.findIndex((b) => b.kind === 'join' && b.ids.includes(id))

/** Node radius and the gap between a node and its label. */
const R = 11
const GAP = 22

/** Name and place, set outside the ring on the side the node faces. */
function Label({ p, lit }: { p: Participant; lit: boolean }) {
  const anchor = p.side === 'right' ? 'start' : p.side === 'left' ? 'end' : 'middle'
  const x = p.side === 'right' ? p.x + GAP : p.side === 'left' ? p.x - GAP : p.x
  /* Two lines: the name, then language · cloud. Top labels stack upward, the rest read downward from the node. */
  const nameY =
    p.side === 'top' ? p.y - GAP - TYPE.label - 6 : p.side === 'bottom' ? p.y + GAP + TYPE.title * 0.72 : p.y - 3
  const placeY = nameY + TYPE.label + 6
  return (
    <g>
      <text
        x={x}
        y={nameY}
        textAnchor={anchor}
        className={cn('font-medium font-sans', lit ? 'fill-hero-accent' : 'fill-foreground')}
        style={{ transition: 'fill 200ms ease' }}
        fontSize={TYPE.title}
      >
        {p.label}
      </text>
      <text x={x} y={placeY} textAnchor={anchor} className="fill-muted-foreground font-sans" fontSize={TYPE.label}>
        {p.place}
      </text>
    </g>
  )
}

type Props = { step: number; cycle: number; active: boolean; className?: string; label: string }

export function OverviewGraph({ step, cycle, active, className, label }: Props) {
  const beat = SCRIPT[step]
  const call = beat.kind === 'call' ? beat : null
  const requestLive = !active || step >= 5
  const joined = (id: string) => (id === 'request' ? requestLive : !active || step >= joinedAt(id))
  const joiningNow = (id: string) => active && beat.kind === 'join' && beat.ids.includes(id)
  const joinDelay = (id: string) => (beat.kind === 'join' && joiningNow(id) ? beat.ids.indexOf(id) * 0.1 : 0)
  const joinedCount = WORKERS.filter((w) => joined(w.id)).length
  const lit = (id: string) => call?.from === id || call?.to === id
  const done = beat.kind === 'done' && active
  const key = `${cycle}-${step}`

  return (
    <svg
      viewBox={`0 ${VIEW.y} ${VIEW.w} ${VIEW.h}`}
      role="img"
      aria-label={label}
      className={cn('h-auto w-full overflow-visible', className)}
    >
      {/* Spokes: each participant's line to the engine, drawn as it registers. */}
      {PARTICIPANTS.map((p) => (
        <Wire
          key={p.id}
          d={`M ${p.x} ${p.y} L ${centre.x} ${centre.y}`}
          visible={joined(p.id)}
          dashed={p.id === 'request' && !requestLive}
          delay={joinDelay(p.id)}
          tone={done || lit(p.id) ? 'lit' : 'idle'}
        />
      ))}

      {/* The call in flight: caller → engine → callee, then the reply back along the same spokes. */}
      {call && active ? (
        <g key={key}>
          <Flow points={route(call.from, call.to)} duration={0.6} />
          {call.to !== 'request' ? (
            <Runner points={[...route(call.from, call.to)].reverse()} delay={0.72} duration={0.42} />
          ) : null}
        </g>
      ) : null}

      <EngineHub
        box={ENGINE_BOX}
        pulseKey={active && (call || done) ? key : undefined}
        lit={Boolean(call) || done}
        environments={joinedCount ? `${joinedCount} worker${joinedCount === 1 ? '' : 's'} registered` : undefined}
      />

      {PARTICIPANTS.map((p) => {
        const on = joined(p.id)
        const hot = lit(p.id) || done
        return (
          <motion.g
            key={p.id}
            initial={false}
            animate={{ opacity: on ? 1 : 0.28, scale: on ? 1 : 0.92 }}
            transition={{ duration: 0.4 * T, delay: on ? joinDelay(p.id) : 0, ease: [0.22, 1, 0.36, 1] }}
            style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
          >
            <circle
              cx={p.x}
              cy={p.y}
              r={R}
              fill="var(--background)"
              stroke={hot ? 'var(--hero-accent)' : 'var(--foreground)'}
              strokeOpacity={hot ? 1 : 0.7}
              strokeWidth={STROKE + 0.25}
              strokeDasharray={on ? undefined : '3 3'}
              style={{ transition: 'stroke 200ms ease, stroke-opacity 200ms ease' }}
            />
            <circle
              cx={p.x}
              cy={p.y}
              r={3.5}
              fill={hot ? 'var(--hero-accent)' : 'var(--foreground)'}
              style={{ transition: 'fill 200ms ease' }}
            />
            <Label p={p} lit={lit(p.id)} />
            {/* A ready ring the moment the worker registers. */}
            {joiningNow(p.id) ? (
              <motion.circle
                key={key}
                cx={p.x}
                cy={p.y}
                r={R}
                fill="none"
                stroke="var(--hero-accent)"
                strokeWidth={STROKE}
                initial={{ opacity: 0.7, scale: 1 }}
                animate={{ opacity: 0, scale: 1.9 }}
                transition={{ duration: 0.7 * T, delay: 0.2 + joinDelay(p.id), ease: [0.22, 1, 0.36, 1] }}
                style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
              />
            ) : null}
          </motion.g>
        )
      })}
    </svg>
  )
}

/**
 * The same graph for phones, stacked top to bottom as HTML: request, engine, then the worker column. Every slot
 * is always present (dimmed until it joins) so the block keeps one height, and it shares the beats with the SVG.
 */
export function OverviewStack({ step, active, className }: { step: number; active: boolean; className?: string }) {
  const beat = SCRIPT[step]
  const call = beat.kind === 'call' ? beat : null
  const joined = (id: string) => !active || step >= joinedAt(id)
  const joinedCount = WORKERS.filter((w) => joined(w.id)).length
  const requestLive = !active || step >= 5
  const lit = (id: string) => call?.from === id || call?.to === id
  const done = beat.kind === 'done' && active
  const slot = (on: boolean, hot: boolean) =>
    cn(
      'flex min-w-0 items-center gap-3 rounded-[10px] border bg-card px-3.5 py-2.5 transition-[opacity,border-color,background-color] duration-200',
      hot ? 'border-hero-accent bg-hero-accent/8' : done ? 'border-hero-accent/50' : 'border-line',
      on ? 'opacity-100' : 'border-dashed opacity-40',
    )
  const rail = (on: boolean) =>
    cn('ml-5 h-3 w-px transition-colors duration-200', on || done ? 'bg-hero-accent' : 'bg-line')

  return (
    <div className={cn('flex flex-col font-sans text-[13px]', className)}>
      <div className={slot(requestLive, lit('request'))}>
        <span className="font-medium text-foreground">Request</span>
      </div>
      <div className={rail(lit('request'))} />
      <div className={cn(slot(true, Boolean(call) || done), !call && !done && 'border-foreground/35')}>
        <span aria-hidden className="flex items-end gap-px">
          {[0, 1, 2].map((i) => (
            <span key={i} className="flex flex-col items-center gap-px">
              <span className="size-[3px] bg-foreground" />
              <span className="h-2 w-[3px] bg-foreground" />
            </span>
          ))}
        </span>
        <span className="font-medium text-foreground">iii engine</span>
        <span className="ml-auto truncate text-[12px] text-muted-foreground tabular-nums">
          {joinedCount ? `${joinedCount} registered` : 'empty'}
        </span>
      </div>
      <div className={rail(Boolean(call))} />
      <ol className="flex flex-col gap-1.5">
        {WORKERS.map((w) => (
          <li key={w.id} className={slot(joined(w.id), lit(w.id))}>
            <span className="flex min-w-0 flex-1 flex-col gap-0.5">
              <span className="truncate font-medium text-foreground">{w.id}</span>
              <span className="truncate font-mono text-[12px] text-muted-foreground">{w.fn}</span>
            </span>
            <span className="shrink-0 text-[12px] text-muted-foreground">{placeOf(w)}</span>
          </li>
        ))}
      </ol>
    </div>
  )
}
