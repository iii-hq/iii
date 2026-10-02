import type { CSSProperties } from 'react'

import { IconBot, IconCode, IconQueue, IconSwap, IconTickSquare, IconTimeCircle } from '@/components/icons/iconly'
import { Logo } from '@/components/site/logo'
import { cn } from '@/lib/utils'
import { ProblemMotion } from './problem-motion'
import styles from './problem-motion.module.css'

/*
 * Both diagrams share one 480×320 coordinate space. Wires are SVG (1px, non-scaling); every box and label is
 * HTML positioned in percentages of that space, so borders, icons, and text land on whole pixels at any width.
 */
const W = 480
const H = 320
const at = (x: number, y: number, w: number, h: number): CSSProperties => ({
  left: `${(x / W) * 100}%`,
  top: `${(y / H) * 100}%`,
  width: `${(w / W) * 100}%`,
  height: `${(h / H) * 100}%`,
})

const services = [
  { x: 84, label: 'APIs', Icon: IconCode },
  { x: 240, label: 'Queues', Icon: IconQueue },
  { x: 396, label: 'Jobs', Icon: IconTimeCircle },
] as const

const routes = {
  outside: [
    { path: 'M240 68v44', delay: '0s' },
    { path: 'M116 184v18q0 12-12 12h-8q-12 0-12 12v22', delay: '1.8s' },
    { path: 'M240 184v64', delay: '2.9s' },
    { path: 'M364 184v18q0 12 12 12h8q12 0 12 12v22', delay: '4s' },
  ],
  inside: [
    { path: 'M240 68v40', delay: '0s' },
    { path: 'M240 188v14q0 12-12 12H96q-12 0-12 12v22', delay: '1.8s' },
    { path: 'M240 188v60', delay: '1.8s' },
    { path: 'M240 188v14q0 12 12 12h132q12 0 12 12v22', delay: '1.8s' },
  ],
} as const

const connectionPoints = [
  { x: 240, y: 88 },
  { x: 84, y: 248 },
  { x: 240, y: 248 },
  { x: 396, y: 248 },
]

function Wires({ connected }: { connected: boolean }) {
  const paths = routes[connected ? 'inside' : 'outside']
  return (
    <svg viewBox={`0 0 ${W} ${H}`} aria-hidden className="absolute inset-0 h-full w-full overflow-visible">
      <g fill="none" vectorEffect="non-scaling-stroke">
        <g stroke="var(--line-strong)" strokeWidth="1" strokeDasharray={connected ? undefined : '4 5'}>
          {paths.map(({ path }) => (
            <path key={path} d={path} vectorEffect="non-scaling-stroke" />
          ))}
        </g>
        <g
          stroke={connected ? 'var(--hero-accent)' : 'var(--foreground)'}
          strokeOpacity={connected ? 1 : 0.45}
          strokeWidth="2"
          strokeLinecap="round"
        >
          {paths.map(({ path, delay }) => (
            <path
              key={path}
              d={path}
              pathLength="100"
              vectorEffect="non-scaling-stroke"
              className={styles.packet}
              style={{ animationDelay: delay }}
            />
          ))}
        </g>
        {connected ? (
          <g fill="var(--foreground)" stroke="var(--background)" strokeWidth="2">
            {connectionPoints.map(({ x, y }) => (
              <circle key={`${x}-${y}`} cx={x} cy={y} r="3" vectorEffect="non-scaling-stroke" />
            ))}
          </g>
        ) : null}
      </g>
    </svg>
  )
}

/** A node box with an optional pulse ring that lights when a request arrives. */
function Node({
  style,
  className,
  pulse,
  pulseDelay,
  children,
}: {
  style: CSSProperties
  className?: string
  pulse?: 'accent' | 'soft'
  pulseDelay?: string
  children: React.ReactNode
}) {
  return (
    <div style={style} className={cn('absolute flex items-center justify-center rounded-[9px] border', className)}>
      {pulse ? (
        <span
          aria-hidden
          className={cn(
            styles.pulse,
            'absolute -inset-px rounded-[inherit] border',
            pulse === 'accent' ? 'border-hero-accent' : 'border-foreground/45',
          )}
          style={{ animationDelay: pulseDelay }}
        />
      ) : null}
      {children}
    </div>
  )
}

function Label({ icon: Icon, children, muted }: { icon: typeof IconBot; children: string; muted?: boolean }) {
  return (
    <span className="flex items-center gap-2 text-[13px] text-foreground leading-none">
      <Icon className={cn('size-4', muted ? 'text-muted-foreground' : 'text-foreground')} />
      {children}
    </span>
  )
}

function Diagram({ connected }: { connected: boolean }) {
  return (
    <div
      role="img"
      aria-label={
        connected
          ? 'An agent and services connect to the iii engine, sharing functions, permissions, and a trace.'
          : 'An agent reaches APIs, queues, and jobs through a separate tool layer.'
      }
      className="relative w-full"
      style={{ aspectRatio: `${W} / ${H}` }}
    >
      <Wires connected={connected} />

      <Node style={at(164, 20, 152, 48)} className="border-line-strong bg-background">
        <Label icon={IconBot} muted>
          AI agent
        </Label>
      </Node>

      {connected ? <Engine /> : <Layer />}

      {services.map(({ x, label, Icon }, index) => (
        <Node
          key={label}
          style={at(x - 60, 248, 120, 48)}
          className={cn('bg-background', connected ? 'border-line-strong' : 'border-line')}
          pulse={connected ? 'accent' : 'soft'}
          pulseDelay={connected ? '3.3s' : `${3.3 + index * 1.1}s`}
        >
          <Label icon={Icon} muted={!connected}>
            {label}
          </Label>
        </Node>
      ))}
    </div>
  )
}

/** Outside: the hand-written tool layer between the agent and the system. */
function Layer() {
  return (
    <>
      <div style={at(24, 104, 432, 88)} className="absolute rounded-xl bg-faint" />
      <Node
        style={at(32, 112, 416, 72)}
        className="flex-col gap-2.5 rounded-lg border-dashed border-line-strong bg-background"
        pulse="soft"
        pulseDelay="1.3s"
      >
        <span className="font-mono text-[10px] text-muted-foreground uppercase tracking-[0.12em] leading-none">
          A layer you maintain
        </span>
        <span className="flex items-center font-mono text-[12px] text-foreground leading-none">
          <span className="px-5">MCP</span>
          <span aria-hidden className="h-4 w-px bg-line" />
          <span className="px-5">HTTP tools</span>
          <span aria-hidden className="h-4 w-px bg-line" />
          <span className="px-5">Shell</span>
        </span>
      </Node>
    </>
  )
}

/** Inside: the engine every worker connects to. */
function Engine() {
  return (
    <>
      <div style={at(168, 100, 144, 96)} className="absolute rounded-[17px] bg-foreground/[0.04]" />
      <Node
        style={at(176, 108, 128, 80)}
        className="rounded-xl border-foreground/30 bg-background"
        pulse="accent"
        pulseDelay="1.3s"
      >
        <span className="flex h-[80%] w-[87.5%] items-center justify-center rounded-lg border border-foreground/70 bg-background">
          <Logo className="h-9 w-auto text-foreground" />
        </span>
      </Node>
      <div
        style={{ left: `${(306 / W) * 100}%`, top: `${(150 / H) * 100}%` }}
        className="absolute flex -translate-y-1/2 items-center gap-2.5 whitespace-nowrap"
      >
        <span aria-hidden className="h-px w-3.5 shrink-0 bg-line-strong" />
        <span className="flex flex-col gap-1 font-mono text-[11px] leading-none">
          <span className="text-muted-foreground">one engine</span>
          <span className="text-foreground">shared functions</span>
        </span>
      </div>
    </>
  )
}

/** Both architectures remain visible while requests animate along their connections. */
export function ProblemLayer() {
  return (
    <ProblemMotion>
      <div className="grid min-w-0 gap-10 min-[900px]:grid-cols-2 min-[900px]:gap-0">
        <Comparison connected={false} />
        <Comparison connected />
      </div>
    </ProblemMotion>
  )
}

function Comparison({ connected }: { connected: boolean }) {
  return (
    <figure
      className={cn(
        'min-w-0',
        connected
          ? 'min-[900px]:border-l min-[900px]:border-dashed min-[900px]:pl-10 lg:pl-12'
          : 'min-[900px]:pr-10 lg:pr-12',
      )}
    >
      <figcaption>
        <div className="flex items-center gap-2 font-mono text-[12px] uppercase tracking-[0.08em]">
          <span
            aria-hidden
            className={cn('size-1.5 rounded-full', connected ? 'bg-foreground' : 'bg-muted-foreground')}
          />
          <span className={connected ? 'text-foreground' : 'text-muted-foreground'}>
            {connected ? 'With iii' : 'Without iii'}
          </span>
        </div>
        <h3
          className={cn(
            'mt-3 text-balance font-medium text-xl tracking-tight md:text-2xl',
            connected ? 'text-foreground' : 'text-muted-foreground',
          )}
        >
          {connected ? 'Part of the system.' : 'Outside, looking in.'}
        </h3>
        <p className="mt-2 max-w-sm text-pretty text-[15px] text-muted-foreground leading-relaxed">
          {connected
            ? 'Agents call live functions, just like any service.'
            : 'Agents only see the tools someone wrote for them.'}
        </p>
      </figcaption>
      <div className="relative mx-auto mt-6 max-w-[560px]">
        <div
          aria-hidden
          className="bg-dots pointer-events-none absolute inset-0 opacity-60 [mask-image:radial-gradient(ellipse_at_center,black,transparent_75%)]"
        />
        <Diagram connected={connected} />
      </div>
      <div
        className={cn(
          'mt-4 flex items-start gap-2.5 border-t border-dashed pt-4 text-[13px]',
          connected ? 'text-foreground' : 'text-muted-foreground',
        )}
      >
        {connected ? (
          <IconTickSquare className="mt-px size-4 shrink-0 text-foreground" />
        ) : (
          <IconSwap className="mt-px size-4 shrink-0" />
        )}
        <p>{connected ? 'Shared permissions. One trace.' : 'Separate retries. Fragmented traces.'}</p>
      </div>
    </figure>
  )
}
