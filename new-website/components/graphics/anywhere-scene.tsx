'use client'

import { AnimatePresence, motion } from 'motion/react'
import { type ComponentType, useEffect, useState } from 'react'
import { easeOut } from '@/lib/motion'

import { cn } from '@/lib/utils'
import { BrowserTab, CiRunner, type DeviceState, GpuBox, MacSimulators, MicroVm, PiBoard } from './anywhere-devices'
import { useGraphicLoop } from './use-graphic-loop'

type P = { x: number; y: number }
type Box = { x: number; y: number; w: number; h: number }
/** A drawable link: exact `d` for the resting line plus sampled points (device/near side first) for packets. */
type Link = { d: string; pts: P[] }

export type DeviceId = 'pi' | 'mac' | 'gpu' | 'browser' | 'ci' | 'vm'
type LinkId = DeviceId | 'bridgeA' | 'bridgeB' | 'remote'

const ease = easeOut
const STEP_MS = 2600

// ---------- geometry helpers ----------

function bezier(p0: P, c1: P, c2: P, p3: P, n = 24): P[] {
  return Array.from({ length: n + 1 }, (_, i) => {
    const t = i / n
    const u = 1 - t
    return {
      x: u * u * u * p0.x + 3 * u * u * t * c1.x + 3 * u * t * t * c2.x + t * t * t * p3.x,
      y: u * u * u * p0.y + 3 * u * u * t * c1.y + 3 * u * t * t * c2.y + t * t * t * p3.y,
    }
  })
}

/** Vertical S-curve from `a` to `b`. */
function sCurve(a: P, b: P): Link {
  const m = (a.y + b.y) / 2
  if (a.x === b.x) return straight(a, b)
  return {
    d: `M${a.x} ${a.y} C${a.x} ${m} ${b.x} ${m} ${b.x} ${b.y}`,
    pts: bezier(a, { x: a.x, y: m }, { x: b.x, y: m }, b),
  }
}

function straight(a: P, b: P): Link {
  return { d: `M${a.x} ${a.y} L${b.x} ${b.y}`, pts: [a, b] }
}

/** Horizontal run from `a` to column `lane`, a rounded corner, then vertical to `endY`. */
function elbow(a: P, lane: number, endY: number, r = 8): Link {
  const sx = Math.sign(lane - a.x)
  const sy = Math.sign(endY - a.y)
  const c0 = { x: lane - sx * r, y: a.y }
  const c1 = { x: lane, y: a.y + sy * r }
  const corner = bezier(c0, { x: lane, y: a.y }, { x: lane, y: a.y }, c1, 6)
  return {
    d: `M${a.x} ${a.y} H${c0.x} Q${lane} ${a.y} ${c1.x} ${c1.y} V${endY}`,
    pts: [a, ...corner, { x: lane, y: endY }],
  }
}

function polyline(pts: P[]) {
  return pts.map((p, i) => `${i ? 'L' : 'M'}${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(' ')
}

/** Keyframe times proportional to arc length, so packets move at constant speed. */
function times(pts: P[]) {
  const acc = [0]
  for (let i = 1; i < pts.length; i++)
    acc.push(acc[i - 1] + Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y))
  const total = acc[acc.length - 1] || 1
  return acc.map((v) => v / total)
}

// ---------- devices ----------

type DeviceDef = {
  id: DeviceId
  name: string
  fn: string
  Art: ComponentType<DeviceState>
  ephemeral?: boolean
}

export const DEVICES: DeviceDef[] = [
  { id: 'pi', name: 'raspberry-pi', fn: 'sensors::read', Art: PiBoard },
  { id: 'mac', name: 'mac-studio', fn: 'simulator::boot', Art: MacSimulators },
  { id: 'gpu', name: 'gpu-box', fn: 'llm::generate', Art: GpuBox },
  { id: 'browser', name: 'browser-tab', fn: 'tab::notify', Art: BrowserTab },
  { id: 'ci', name: 'ci-runner', fn: 'ci::test', Art: CiRunner, ephemeral: true },
  { id: 'vm', name: 'microvm', fn: 'sandbox::exec', Art: MicroVm },
]

// ---------- layouts ----------

type Placed = { x: number; y: number; s: number; label: 'above' | 'below'; link: Link }
type Layout = {
  w: number
  h: number
  engineA: Box
  engineB: Box
  zone: Box
  bridge: Box
  links: Record<'bridgeA' | 'bridgeB' | 'remote', Link>
  /** Workers attached to the second engine; the first one's link is `links.remote`. */
  remotes: (P & { hw: number; kind: string; label: string; link: Link })[]
  devices: Record<DeviceId, Placed>
  /** Show JSON frames as labelled pills (desktop) or plain packets (mobile). */
  pills: boolean
}

function desktop(): Layout {
  const A: Box = { x: 260, y: 220, w: 240, h: 168 }
  const port = (cx: number) => 380 + (cx - 380) * 0.35
  const top = (id: DeviceId, cx: number): [DeviceId, Placed] => [
    id,
    { x: cx - 60, y: 60, s: 1, label: 'above', link: sCurve({ x: cx, y: 136 }, { x: port(cx), y: A.y }) },
  ]
  const bottom = (id: DeviceId, cx: number): [DeviceId, Placed] => [
    id,
    { x: cx - 60, y: 484, s: 1, label: 'below', link: sCurve({ x: cx, y: 484 }, { x: port(cx), y: A.y + A.h }) },
  ]
  const B: Box = { x: 855, y: 278, w: 150, h: 64 }
  const orders = straight({ x: 930, y: 222 }, { x: 930, y: B.y })
  const bridge: Box = { x: 628, y: 296, w: 92, h: 28 }
  return {
    w: 1100,
    h: 612,
    engineA: A,
    engineB: B,
    zone: { x: 776, y: 140, w: 308, h: 340 },
    bridge,
    links: {
      bridgeA: straight({ x: A.x + A.w, y: 310 }, { x: bridge.x, y: 310 }),
      bridgeB: straight({ x: bridge.x + bridge.w, y: 310 }, { x: B.x, y: 310 }),
      remote: orders,
    },
    remotes: [
      { x: 930, y: 206, hw: 58, kind: 'ts', label: 'orders-api', link: orders },
      {
        x: 930,
        y: 414,
        hw: 58,
        kind: 'py',
        label: 'billing.py',
        link: straight({ x: 930, y: 398 }, { x: 930, y: B.y + B.h }),
      },
    ],
    devices: Object.fromEntries([
      top('pi', 140),
      top('mac', 380),
      top('gpu', 620),
      bottom('browser', 140),
      bottom('ci', 380),
      bottom('vm', 620),
    ]) as Record<DeviceId, Placed>,
    pills: true,
  }
}

function mobile(): Layout {
  const A: Box = { x: 60, y: 196, w: 240, h: 168 }
  const s = 0.9
  const bottomY = A.y + A.h
  const rows = [426, 566, 706]
  const inner = { l: 36 + 120 * s, r: 216 }
  const mid = (row: number) => row + (76 * s) / 2
  const place = (id: DeviceId, col: 'l' | 'r', row: number, link: Link): [DeviceId, Placed] => [
    id,
    { x: col === 'l' ? 36 : 216, y: row, s, label: 'below', link },
  ]
  const B: Box = { x: 184, y: 44, w: 112, h: 44 }
  const orders = straight({ x: 130, y: 66 }, { x: B.x, y: 66 })
  const bridge: Box = { x: 200, y: 128, w: 80, h: 26 }
  return {
    w: 360,
    h: 842,
    engineA: A,
    engineB: B,
    zone: { x: 8, y: 8, w: 344, h: 100 },
    bridge,
    links: {
      bridgeA: straight({ x: 240, y: A.y }, { x: 240, y: bridge.y + bridge.h }),
      bridgeB: straight({ x: 240, y: bridge.y }, { x: 240, y: B.y + B.h }),
      remote: orders,
    },
    remotes: [{ x: 80, y: 66, hw: 50, kind: 'ts', label: 'orders-api', link: orders }],
    devices: Object.fromEntries([
      place('pi', 'l', rows[0], sCurve({ x: 90, y: rows[0] }, { x: 116, y: bottomY })),
      place('mac', 'r', rows[0], sCurve({ x: 270, y: rows[0] }, { x: 244, y: bottomY })),
      place('gpu', 'l', rows[1], elbow({ x: inner.l, y: mid(rows[1]) }, 164, bottomY)),
      place('browser', 'r', rows[1], elbow({ x: inner.r, y: mid(rows[1]) }, 196, bottomY)),
      place('ci', 'l', rows[2], elbow({ x: inner.l, y: mid(rows[2]) }, 176, bottomY)),
      place('vm', 'r', rows[2], elbow({ x: inner.r, y: mid(rows[2]) }, 184, bottomY)),
    ]) as Record<DeviceId, Placed>,
    pills: false,
  }
}

const LAYOUTS = { desktop: desktop(), mobile: mobile() }

// ---------- script ----------

type Seg = { link: LinkId; rev?: boolean; frame: string; at?: number }
type Step = {
  segs: Seg[]
  /** Device that starts working once the first frame arrives. */
  target?: DeviceId
  arrive?: number
  ci: boolean
  gpu: 'up' | 'crash' | 'down'
  /** Seconds before the registry reflects this step. */
  regDelay?: number
  route: string
  detail: string
  tone?: 'fail'
}

const SCRIPT: Step[] = [
  {
    segs: [
      { link: 'browser', frame: 'invokefunction' },
      { link: 'gpu', rev: true, frame: 'invokefunction' },
    ],
    target: 'gpu',
    arrive: 1.2,
    ci: false,
    gpu: 'up',
    route: 'browser-tab → engine → gpu-box',
    detail: '{"type":"invokefunction","function_id":"llm::generate"}',
  },
  {
    segs: [{ link: 'ci', frame: 'registerfunction', at: 0.7 }],
    ci: true,
    gpu: 'up',
    regDelay: 1.3,
    route: 'ci-runner joined',
    detail: '{"type":"registerfunction","id":"ci::test"}',
  },
  {
    segs: [
      { link: 'ci', rev: true, frame: 'invokefunction' },
      { link: 'ci', frame: 'invocationresult', at: 1.85 },
    ],
    target: 'ci',
    arrive: 0.6,
    ci: true,
    gpu: 'up',
    route: 'engine → ci-runner',
    detail: '{"type":"invokefunction","function_id":"ci::test"}',
  },
  {
    segs: [],
    ci: false,
    gpu: 'up',
    regDelay: 0.5,
    route: 'ci-runner left',
    detail: 'ci::test removed from the registry',
  },
  {
    segs: [
      { link: 'remote', frame: 'invokefunction' },
      { link: 'bridgeB', rev: true, frame: 'invokefunction' },
      { link: 'bridgeA', rev: true, frame: 'invokefunction' },
      { link: 'pi', rev: true, frame: 'invokefunction' },
    ],
    target: 'pi',
    arrive: 2.1,
    ci: false,
    gpu: 'up',
    route: 'orders-api → bridge → raspberry-pi',
    detail: '{"type":"invokefunction","function_id":"sensors::read"}',
  },
  {
    segs: [],
    ci: false,
    gpu: 'crash',
    regDelay: 1.1,
    route: 'gpu-box crashed',
    detail: 'llm::generate dropped from the registry',
    tone: 'fail',
  },
  {
    segs: [
      { link: 'vm', frame: 'invokefunction' },
      { link: 'mac', rev: true, frame: 'invokefunction' },
    ],
    target: 'mac',
    arrive: 1.2,
    ci: false,
    gpu: 'down',
    route: 'microvm → engine → mac-studio',
    detail: '{"type":"invokefunction","function_id":"simulator::boot"}',
  },
  {
    segs: [{ link: 'gpu', frame: 'registerfunction', at: 0.5 }],
    ci: false,
    gpu: 'up',
    regDelay: 1.1,
    route: 'gpu-box reconnected',
    detail: '{"type":"registerfunction","id":"llm::generate"}',
  },
]

const STATIC: Step = {
  segs: [],
  ci: true,
  gpu: 'up',
  route: 'one WebSocket per worker',
  detail: 'JSON frames: registerfunction · invokefunction · invocationresult',
}

// ---------- scene ----------

type SceneProps = {
  hovered: DeviceId | null
  onHover: (id: DeviceId | null) => void
  className?: string
}

/**
 * "Runs anywhere": six hand-drawn devices each hold a WebSocket to the engine and appear in its
 * registry. JSON frames travel the lines, a CI runner joins and leaves, a GPU box crashes and
 * drops out while the rest keep serving, and a `bridge` worker links a second engine on another network.
 */
export function AnywhereScene({ hovered, onHover, className }: SceneProps) {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const [step, setStep] = useState(0)
  const [regStep, setRegStep] = useState(0)
  const [arrived, setArrived] = useState(-1)

  useEffect(() => {
    if (!active) return
    const id = window.setInterval(() => setStep((s) => s + 1), STEP_MS)
    return () => window.clearInterval(id)
  }, [active])

  useEffect(() => {
    if (!active) return
    const s = SCRIPT[step % SCRIPT.length]
    const t1 = window.setTimeout(() => setRegStep(step), (s.regDelay ?? 0) * 1000)
    const t2 = window.setTimeout(() => setArrived(step), (s.arrive ?? 0) * 1000)
    return () => {
      window.clearTimeout(t1)
      window.clearTimeout(t2)
    }
  }, [active, step])

  const cur = active ? SCRIPT[step % SCRIPT.length] : STATIC
  const reg = active ? SCRIPT[regStep % SCRIPT.length] : STATIC
  const busy = active && arrived === step ? cur.target : undefined

  const view = { step, cur, reg, busy, active, hovered, onHover }

  return (
    <div ref={ref} className={cn('relative', className)}>
      <SceneSvg layout={LAYOUTS.desktop} {...view} className="hidden md:block" />
      <SceneSvg layout={LAYOUTS.mobile} {...view} className="md:hidden" />
      <div className="mt-3 flex min-h-10 flex-col items-center justify-center gap-0.5 px-4 text-center font-mono text-[12px] md:flex-row md:gap-3 md:text-xs">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={active ? step % SCRIPT.length : 'static'}
            className="flex flex-col items-center gap-0.5 md:flex-row md:gap-3"
            initial={{ opacity: 0, y: 6, filter: 'blur(2px)' }}
            animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
            exit={{ opacity: 0, y: -6, filter: 'blur(2px)' }}
            transition={{ duration: 0.3, ease }}
          >
            <span className="flex items-center gap-2 text-muted-foreground">
              {cur.tone === 'fail' ? <span className="size-1.5 rounded-full bg-fail" /> : null}
              {cur.route}
            </span>
            <span className="break-all text-foreground">{cur.detail}</span>
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  )
}

type SvgProps = {
  layout: Layout
  step: number
  cur: Step
  reg: Step
  busy?: DeviceId
  active: boolean
  hovered: DeviceId | null
  onHover: (id: DeviceId | null) => void
  className?: string
}

function connected(id: DeviceId, s: Step) {
  if (id === 'ci') return s.ci
  if (id === 'gpu') return s.gpu === 'up'
  return true
}

function SceneSvg({ layout: L, step, cur, reg, busy, active, hovered, onHover, className }: SvgProps) {
  const linkOf = (id: LinkId): Link =>
    id in L.devices ? L.devices[id as DeviceId].link : L.links[id as keyof Layout['links']]
  const inRegistry = DEVICES.filter((d) => connected(d.id, reg))
  const n = cur.segs.length
  const dur = n > 2 ? 0.5 : 0.6

  return (
    <svg
      viewBox={`0 0 ${L.w} ${L.h}`}
      role="img"
      aria-label="Six workers on different hardware, a Raspberry Pi, a Mac hosting iOS simulators, a GPU box, a browser tab, a CI runner and a microVM, each hold one WebSocket to the engine and appear in its registry. A bridge worker links the engine to a second engine on another network."
      className={cn('h-auto w-full overflow-visible', className)}
    >
      {/* Remote network with a second engine */}
      <rect
        x={L.zone.x}
        y={L.zone.y}
        width={L.zone.w}
        height={L.zone.h}
        rx={14}
        fill="none"
        stroke="var(--line-strong)"
        strokeDasharray="3 4"
      />
      <text
        x={L.zone.x + 14}
        y={L.zone.y + 20}
        className="fill-muted-foreground font-mono"
        fontSize={10}
        letterSpacing="0.08em"
      >
        NETWORK B
      </text>

      {/* Resting links */}
      {[L.links.bridgeA, L.links.bridgeB, ...L.remotes.map((r) => r.link)].map((l) => (
        <path key={l.d} d={l.d} fill="none" stroke="var(--line-strong)" />
      ))}
      {DEVICES.map((d) => {
        const on = connected(d.id, cur)
        return (
          <motion.path
            key={d.id}
            d={L.devices[d.id].link.d}
            fill="none"
            stroke={hovered === d.id ? 'var(--hero-accent)' : 'var(--line-strong)'}
            strokeDasharray={d.ephemeral ? '3 4' : undefined}
            initial={false}
            animate={{ pathLength: on ? 1 : 0, opacity: on ? 1 : 0 }}
            transition={{ duration: 0.7, ease, delay: cur.gpu === 'crash' && d.id === 'gpu' ? 0.4 : 0 }}
            style={{ transition: 'stroke 200ms ease' }}
          />
        )
      })}

      {/* Active route */}
      <AnimatePresence>
        {active && n ? (
          <motion.g key={`route-${step}`} exit={{ opacity: 0 }} transition={{ duration: 0.4 }}>
            {cur.segs.map((s, i) => {
              const pts = s.rev ? [...linkOf(s.link).pts].reverse() : linkOf(s.link).pts
              return (
                <motion.path
                  key={`${s.link}-${i}`}
                  d={polyline(pts)}
                  fill="none"
                  stroke="var(--hero-accent)"
                  strokeWidth={1.25}
                  initial={{ pathLength: 0, opacity: 0.9 }}
                  animate={{ pathLength: 1, opacity: [0.9, 0.9, 0.2] }}
                  transition={{
                    pathLength: { duration: dur, delay: s.at ?? i * (dur + 0.02), ease: 'linear' },
                    opacity: { duration: 1.6, delay: s.at ?? i * (dur + 0.02), times: [0, 0.6, 1] },
                  }}
                />
              )
            })}
          </motion.g>
        ) : null}
      </AnimatePresence>

      {/* Bridge worker */}
      <rect
        x={L.bridge.x}
        y={L.bridge.y}
        width={L.bridge.w}
        height={L.bridge.h}
        rx={L.bridge.h / 2.6}
        fill="var(--node)"
        stroke="var(--line-strong)"
      />
      <text
        x={L.bridge.x + L.bridge.w / 2}
        y={L.bridge.y + L.bridge.h / 2 + 3.5}
        textAnchor="middle"
        className="fill-foreground font-mono"
        fontSize={11}
      >
        bridge
      </text>

      {/* Workers on the second engine */}
      {L.remotes.map((r) => (
        <g key={r.label}>
          <rect
            x={r.x - r.hw}
            y={r.y - 16}
            width={r.hw * 2}
            height={32}
            rx={10}
            fill="var(--node)"
            stroke="var(--line-strong)"
          />
          <rect x={r.x - r.hw + 7} y={r.y - 9} width={18} height={18} rx={5} fill="var(--faint)" stroke="var(--line)" />
          <text
            x={r.x - r.hw + 16}
            y={r.y + 3}
            textAnchor="middle"
            className="fill-muted-foreground font-mono"
            fontSize={8.5}
          >
            {r.kind}
          </text>
          <text x={r.x - r.hw + 31} y={r.y + 4} className="fill-foreground font-mono" fontSize={11}>
            {r.label}
          </text>
        </g>
      ))}

      <EngineBox box={L.engineB} compact />

      {/* Engine A with its live registry */}
      <EngineBox box={L.engineA} count={inRegistry.length}>
        {DEVICES.map((d) => {
          const slot = inRegistry.findIndex((r) => r.id === d.id)
          const present = slot >= 0
          const failing = d.id === 'gpu' && cur.gpu === 'crash' && present
          const y = L.engineA.y + 54 + (present ? slot : Math.max(0, inRegistry.length - 0.5)) * 20
          return (
            <motion.g
              key={d.id}
              initial={false}
              animate={{ opacity: present ? 1 : 0, y }}
              transition={{ duration: 0.5, ease }}
            >
              <circle
                cx={L.engineA.x + 17}
                cy={-3.5}
                r={2.5}
                fill={failing ? 'var(--fail)' : 'var(--ok)'}
                style={{ transition: 'fill 200ms ease' }}
              />
              <text
                x={L.engineA.x + 28}
                y={0}
                className={cn('font-mono', hovered === d.id ? 'fill-foreground' : 'fill-foreground/85')}
                fontSize={11}
                textDecoration={failing ? 'line-through' : undefined}
              >
                {d.fn}
              </text>
              <text
                x={L.engineA.x + L.engineA.w - 14}
                y={0}
                textAnchor="end"
                className="fill-muted-foreground font-mono"
                fontSize={10}
              >
                {d.name}
              </text>
            </motion.g>
          )
        })}
      </EngineBox>

      {/* Devices */}
      {DEVICES.map((d) => {
        const p = L.devices[d.id]
        const on = connected(d.id, cur)
        const down = d.id === 'gpu' && cur.gpu !== 'up'
        const cx = p.x + 60 * p.s
        const artH = 76 * p.s
        const nameY = p.label === 'above' ? p.y - 26 : p.y + artH + 20
        const Art = d.Art
        return (
          // biome-ignore lint/a11y/noStaticElementInteractions: hover mirrors the bullet list; purely visual.
          <g key={d.id} onMouseEnter={() => onHover(d.id)} onMouseLeave={() => onHover(null)}>
            <rect
              x={p.x - 10}
              y={p.label === 'above' ? nameY - 18 : p.y - 8}
              width={120 * p.s + 20}
              height={artH + 52}
              rx={12}
              fill={hovered === d.id ? 'var(--faint)' : 'transparent'}
              stroke={hovered === d.id ? 'var(--line)' : 'transparent'}
              style={{ transition: 'fill 200ms ease, stroke 200ms ease' }}
            />
            <motion.g
              initial={false}
              animate={{ opacity: on ? 1 : down ? 0.45 : 0.22 }}
              transition={{ duration: 0.6, ease, delay: cur.gpu === 'crash' && d.id === 'gpu' ? 0.3 : 0 }}
            >
              <g transform={`translate(${p.x} ${p.y}) scale(${p.s})`}>
                <Art busy={busy === d.id} down={down} />
              </g>
              <text x={cx} y={nameY} textAnchor="middle" className="fill-foreground font-mono" fontSize={12}>
                {d.name}
              </text>
              <text
                x={cx}
                y={nameY + 16}
                textAnchor="middle"
                className="fill-muted-foreground font-mono"
                fontSize={10.5}
              >
                {d.fn}
              </text>
            </motion.g>
            <AnimatePresence>
              {down ? <CrashMark key="crash" link={p.link} delay={cur.gpu === 'crash' ? 0.5 : 0} /> : null}
            </AnimatePresence>
          </g>
        )
      })}

      {/* Frames in flight */}
      <AnimatePresence>
        {active && n ? (
          <motion.g key={`frames-${step}`} exit={{ opacity: 0 }}>
            {cur.segs.map((s, i) => {
              const pts = s.rev ? [...linkOf(s.link).pts].reverse() : linkOf(s.link).pts
              return (
                <Frame
                  key={`${s.link}-${i}`}
                  pts={pts}
                  label={L.pills ? s.frame : undefined}
                  delay={s.at ?? i * (dur + 0.02)}
                  duration={dur}
                />
              )
            })}
          </motion.g>
        ) : null}
      </AnimatePresence>
    </svg>
  )
}

/** Red cross just off the device end of a link whose worker has crashed. */
function CrashMark({ link, delay }: { link: Link; delay: number }) {
  const [a, b] = link.pts
  const len = Math.hypot(b.x - a.x, b.y - a.y) || 1
  const x = a.x + ((b.x - a.x) / len) * 14
  const y = a.y + ((b.y - a.y) / len) * 14
  return (
    <motion.path
      d={`M${x - 4} ${y - 4} L${x + 4} ${y + 4} M${x + 4} ${y - 4} L${x - 4} ${y + 4}`}
      stroke="var(--fail)"
      strokeWidth={1.5}
      strokeLinecap="round"
      initial={{ opacity: 0, scale: 0.6 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.3, delay, ease }}
      style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
    />
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

function EngineBox({
  box,
  count,
  compact,
  children,
}: {
  box: Box
  count?: number
  compact?: boolean
  children?: React.ReactNode
}) {
  if (compact) {
    const cy = box.y + box.h / 2
    return (
      <g>
        <rect x={box.x} y={box.y} width={box.w} height={box.h} rx={14} fill="var(--node)" stroke="var(--line-strong)" />
        <Logo x={box.x + box.w / 2 - 34} y={cy - 7.5} s={0.0145} />
        <text
          x={box.x + box.w / 2 - 12}
          y={cy + 3.5}
          className="fill-muted-foreground font-mono"
          fontSize={10}
          letterSpacing="0.12em"
        >
          ENGINE
        </text>
      </g>
    )
  }
  return (
    <g>
      <rect x={box.x} y={box.y} width={box.w} height={box.h} rx={16} fill="var(--node)" stroke="var(--line-strong)" />
      <Logo x={box.x + 14} y={box.y + 10} s={0.0145} />
      <text
        x={box.x + 36}
        y={box.y + 21}
        className="fill-muted-foreground font-mono"
        fontSize={10}
        letterSpacing="0.12em"
      >
        ENGINE
      </text>
      <text
        x={box.x + box.w - 14}
        y={box.y + 21}
        textAnchor="end"
        className="fill-muted-foreground font-mono"
        fontSize={10}
      >
        registry · {count} functions
      </text>
      <line x1={box.x} y1={box.y + 32} x2={box.x + box.w} y2={box.y + 32} stroke="var(--line)" />
      {children}
    </g>
  )
}

function Frame({ pts, label, delay, duration }: { pts: P[]; label?: string; delay: number; duration: number }) {
  const t = times(pts)
  const text = label ? `{"type":"${label}"}` : ''
  const w = text.length * 6.02 + 16
  return (
    <motion.g
      initial={{ x: pts[0].x, y: pts[0].y, opacity: 0 }}
      animate={{ x: pts.map((p) => p.x), y: pts.map((p) => p.y), opacity: [0, 1, 1, 0] }}
      transition={{
        x: { duration, delay, times: t, ease: 'linear' },
        y: { duration, delay, times: t, ease: 'linear' },
        opacity: { duration, delay, times: [0, 0.12, 0.88, 1] },
      }}
    >
      {label ? (
        <>
          <rect x={-w / 2} y={-9} width={w} height={18} rx={9} fill="var(--node)" stroke="var(--hero-accent)" />
          <text y={3.5} textAnchor="middle" className="fill-foreground font-mono" fontSize={10}>
            {text}
          </text>
        </>
      ) : (
        <circle r={3.5} fill="var(--foreground)" />
      )}
    </motion.g>
  )
}
