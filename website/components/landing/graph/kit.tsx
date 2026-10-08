'use client'

import { motion } from 'motion/react'

import { easeInOut, easeOut } from '@/lib/motion'

import { ENGINE, type Point } from './model'

export const ease = easeOut
/** Global tempo multiplier on every transition in the graph. 1 = the base values as written. */
export const T = 1

/** Type sizes in viewBox units. The graph is 1040 wide and renders at about 760px in the slide column, so 16 ≈ 11.7px. */
export const TYPE = { title: 16, sub: 15.5, tag: 15, label: 14.5 } as const
/** Mono advance per character at a given font size (Geist Mono is 0.6em wide). */
export const advance = (size: number) => size * 0.6

/** One stroke weight for the whole drawing. */
export const STROKE = 1.25

type Tone = 'idle' | 'lit' | 'selected' | 'ghost'

/** Neutral 600: a slot that is not live yet still reads as a dashed outline, not as nothing. */
const GHOST = 'oklch(0.439 0 0)'

const stroke: Record<Tone, string> = {
  idle: 'var(--line-strong)',
  lit: 'var(--hero-accent)',
  selected: 'var(--foreground)',
  ghost: GHOST,
}

type NodeProps = {
  cx: number
  cy: number
  w: number
  h: number
  title: string
  sub?: string | string[]
  /** Right-aligned detail on the title line: the language, a duration, or a status word. */
  meta?: string
  metaTone?: 'muted' | 'accent'
  /** 0..1 progress bar along the inside bottom edge (a span). */
  bar?: number
  barLit?: boolean
  tone?: Tone
  dashed?: boolean
  center?: boolean
  visible?: boolean
  delay?: number
  onClick?: () => void
  label?: string
  /** Dimmed but present (the harness stage shows the idle graph at half strength). */
  faded?: boolean
}

/**
 * A worker / function node. Title and every following line share one left edge; the meta sits on the title
 * line's right edge. Single-line nodes can centre their title.
 */
export function Node({
  cx,
  cy,
  w,
  h,
  title,
  sub,
  meta,
  metaTone = 'muted',
  bar,
  barLit,
  tone = 'idle',
  dashed,
  center,
  visible = true,
  delay = 0,
  onClick,
  label,
  faded,
}: NodeProps) {
  const x = cx - w / 2
  const y = cy - h / 2
  const subs = Array.isArray(sub) ? sub : sub ? [sub] : []
  const pad = 16
  const lineH = 18
  const blockH = TYPE.title + subs.length * lineH
  const titleY = cy - blockH / 2 + TYPE.title * 0.78
  const interactive = Boolean(onClick)
  return (
    <motion.g
      initial={false}
      animate={{ opacity: visible ? (faded ? 0.45 : 1) : 0, scale: visible ? 1 : 0.96 }}
      transition={{ duration: 0.4 * T, delay: visible ? delay : 0, ease }}
      style={{ transformBox: 'fill-box', transformOrigin: 'center', pointerEvents: visible ? 'auto' : 'none' }}
      onClick={onClick}
      onKeyDown={
        interactive
          ? (e) => {
              if (e.key !== 'Enter' && e.key !== ' ') return
              e.preventDefault()
              onClick?.()
            }
          : undefined
      }
      tabIndex={interactive ? 0 : undefined}
      role={interactive ? 'button' : undefined}
      aria-label={interactive ? (label ?? title) : undefined}
      className={
        interactive ? 'cursor-pointer outline-none [&:focus-visible>rect:first-of-type]:stroke-foreground' : undefined
      }
    >
      <rect
        x={x}
        y={y}
        width={w}
        height={h}
        rx={8}
        fill={tone === 'lit' ? 'color-mix(in oklch, var(--hero-accent) 8%, var(--node))' : 'var(--node)'}
        stroke={stroke[tone]}
        strokeWidth={STROKE}
        strokeDasharray={dashed ? '4 4' : undefined}
        style={{ transition: 'stroke 300ms ease, fill 300ms ease' }}
      />
      <text
        x={center ? cx : x + pad}
        y={titleY}
        textAnchor={center ? 'middle' : 'start'}
        className="fill-foreground font-mono"
        fontSize={TYPE.title}
      >
        {title}
      </text>
      {meta ? (
        <text
          x={x + w - pad}
          y={titleY}
          textAnchor="end"
          className={
            metaTone === 'accent'
              ? 'fill-hero-accent font-mono tabular-nums'
              : 'fill-muted-foreground font-mono tabular-nums'
          }
          fontSize={TYPE.label}
        >
          {meta}
        </text>
      ) : null}
      {subs.map((line, i) => (
        <text
          key={line}
          x={x + pad}
          y={titleY + (i + 1) * lineH}
          className="fill-muted-foreground font-mono"
          fontSize={TYPE.sub}
        >
          {line}
        </text>
      ))}
      {bar !== undefined ? (
        <rect
          x={x + pad}
          y={y + h - 8}
          width={(w - pad * 2) * bar}
          height={3}
          rx={1.5}
          fill={barLit ? 'var(--hero-accent)' : 'var(--line-strong)'}
          style={{ transition: 'fill 300ms ease, width 500ms ease' }}
        />
      ) : null}
    </motion.g>
  )
}

/** Small mono tag (file name, label), 26 units tall. */
export function Tag({
  x,
  y,
  children,
  anchor = 'start',
  tone = 'muted',
  visible = true,
  delay = 0,
}: {
  x: number
  y: number
  children: string
  anchor?: 'start' | 'middle' | 'end'
  tone?: 'muted' | 'accent' | 'foreground'
  visible?: boolean
  delay?: number
}) {
  const width = Math.round(children.length * advance(TYPE.tag) + 24)
  const left = anchor === 'start' ? x : anchor === 'end' ? x - width : x - width / 2
  return (
    <motion.g
      initial={false}
      animate={{ opacity: visible ? 1 : 0, y: visible ? 0 : 4 }}
      transition={{ duration: visible ? 0.35 * T : 0.15, delay: visible ? delay : 0, ease }}
    >
      <rect
        x={left}
        y={y - 13}
        width={width}
        height={26}
        rx={13}
        fill="var(--background)"
        stroke={tone === 'accent' ? 'var(--hero-accent)' : 'var(--line-strong)'}
        strokeWidth={STROKE}
      />
      <text
        x={left + width / 2}
        y={y + TYPE.tag * 0.36}
        textAnchor="middle"
        fontSize={TYPE.tag}
        className={
          tone === 'accent'
            ? 'fill-hero-accent font-mono'
            : tone === 'foreground'
              ? 'fill-foreground font-mono'
              : 'fill-muted-foreground font-mono'
        }
      >
        {children}
      </text>
    </motion.g>
  )
}

/** A wire drawn with pathLength. */
export function Wire({
  d,
  visible = true,
  dashed,
  tone = 'idle',
  delay = 0,
}: {
  d: string
  visible?: boolean
  dashed?: boolean
  tone?: 'idle' | 'ghost' | 'lit'
  delay?: number
}) {
  return (
    <motion.path
      d={d}
      fill="none"
      stroke={stroke[tone]}
      strokeWidth={STROKE}
      strokeDasharray={dashed ? '3 6' : undefined}
      strokeLinecap="round"
      strokeLinejoin="round"
      initial={false}
      animate={{ pathLength: visible ? 1 : 0, opacity: visible ? 1 : 0 }}
      transition={{ duration: 0.45 * T, delay: visible ? delay : 0, ease }}
      style={{ transition: 'stroke 300ms ease' }}
    />
  )
}

export type FlowTone = 'call' | 'event'
const flowColor: Record<FlowTone, string> = { call: 'var(--hero-accent)', event: 'var(--warn)' }

/** A packet following a polyline at constant speed. */
export function Runner({
  points,
  delay = 0,
  duration = 0.5,
  tone = 'call',
}: {
  points: Point[]
  delay?: number
  duration?: number
  tone?: FlowTone
}) {
  const lengths = points.slice(1).map((p, i) => Math.hypot(p.x - points[i].x, p.y - points[i].y))
  const total = lengths.reduce((a, b) => a + b, 0) || 1
  const times = points.map((_, i) => lengths.slice(0, i).reduce((a, b) => a + b, 0) / total)
  return (
    <motion.circle
      r={4.5}
      fill={flowColor[tone]}
      initial={{ cx: points[0].x, cy: points[0].y, opacity: 0 }}
      animate={{ cx: points.map((p) => p.x), cy: points.map((p) => p.y), opacity: [0, 1, 1, 0] }}
      transition={{
        cx: { duration: duration * T, delay: delay * T, ease: easeInOut, times },
        cy: { duration: duration * T, delay: delay * T, ease: easeInOut, times },
        opacity: { duration: duration * T, delay: delay * T, times: [0, 0.1, 0.9, 1] },
      }}
    />
  )
}

/**
 * A call or event travelling along the wires: the route lights up behind the packet so the eye can follow
 * where it came from and where it went, then settles to a faint trace.
 */
export function Flow({
  points,
  delay = 0,
  duration = 0.5,
  tone = 'call',
}: {
  points: Point[]
  delay?: number
  duration?: number
  tone?: FlowTone
}) {
  const d = points.map((p, i) => `${i ? 'L' : 'M'} ${p.x} ${p.y}`).join(' ')
  return (
    <g>
      <motion.path
        d={d}
        fill="none"
        stroke={flowColor[tone]}
        strokeWidth={STROKE + 0.75}
        strokeLinecap="round"
        strokeLinejoin="round"
        initial={{ pathLength: 0, opacity: 0 }}
        animate={{ pathLength: 1, opacity: [0, 1, 1, 0.35] }}
        transition={{
          pathLength: { duration: duration * T, delay: delay * T, ease: easeInOut },
          opacity: { duration: duration * T * 1.5, delay: delay * T, times: [0, 0.05, 0.6, 1] },
        }}
      />
      <Runner points={points} delay={delay} duration={duration} tone={tone} />
    </g>
  )
}

/** A status dot that pops in on a node's corner. */
export function Dot({ cx, cy, delay = 0 }: { cx: number; cy: number; delay?: number }) {
  return (
    <motion.circle
      cx={cx}
      cy={cy}
      r={4}
      fill="var(--hero-accent)"
      stroke="var(--background)"
      strokeWidth={2}
      initial={{ opacity: 0, scale: 0.4 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.3 * T, delay: delay * T, ease }}
      style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
    />
  )
}

/** The engine hub: the three-bar mark, its name, and the environments it runs across. */
export function EngineHub({
  pulseKey,
  lit,
  environments,
  box = ENGINE,
}: {
  pulseKey?: string | number
  lit?: boolean
  /** Second line, shown once the environments have merged into the engine. */
  environments?: string
  /** Where to draw it; the system graph's engine slot by default. */
  box?: { x: number; y: number; w: number; h: number }
}) {
  const { x, y, w, h } = box
  const left = x - w / 2
  return (
    <g>
      {pulseKey !== undefined ? (
        <motion.rect
          key={pulseKey}
          x={left}
          y={y - h / 2}
          width={w}
          height={h}
          rx={10}
          fill="none"
          stroke="var(--hero-accent)"
          strokeWidth={STROKE}
          initial={{ opacity: 0.6, scale: 1 }}
          animate={{ opacity: 0, scale: 1.12 }}
          transition={{ duration: 0.7 * T, ease: easeOut }}
          style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
        />
      ) : null}
      <rect
        x={left}
        y={y - h / 2}
        width={w}
        height={h}
        rx={10}
        fill="var(--card)"
        stroke={lit ? 'var(--hero-accent)' : 'var(--foreground)'}
        strokeOpacity={lit ? 1 : 0.6}
        strokeWidth={STROKE}
        style={{ transition: 'stroke 300ms ease' }}
      />
      <g transform={`translate(${left + 20} ${y - (environments ? 20 : 11)})`} className="fill-foreground">
        {[0, 8, 16].map((dx) => (
          <g key={dx}>
            <rect x={dx} y={0} width={5} height={5} />
            <rect x={dx} y={7} width={5} height={15} />
          </g>
        ))}
      </g>
      <text
        x={left + 52}
        y={y + (environments ? -3 : TYPE.title * 0.36)}
        className="fill-foreground font-mono"
        fontSize={TYPE.title}
      >
        iii engine
      </text>
      {environments ? (
        <motion.text
          key="env"
          x={left + 52}
          y={y + 17}
          className="fill-muted-foreground font-mono"
          fontSize={13.5}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.3 * T }}
        >
          {environments}
        </motion.text>
      ) : null}
    </g>
  )
}
