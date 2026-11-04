'use client'

import { useId } from 'react'

import {
  backOut,
  clamp01,
  easeIn,
  easeInOut,
  easeOut,
  lerp,
  mix,
  pop,
  scaleAt,
  seg,
  useClock,
  useTimeline,
} from '../shared/anim'
import { COLS, colX, INK, type MarkProps, MarkSvg } from '../shared/mark-svg'

/*
 * Diwali takes on the iii mark. At t = 0 every idea draws exactly the plain mark.
 *
 * Two things make these read at the header's 24px, where the dots are 6px squares: the stems take part (light and
 * colour wash down them from whatever is happening on the dots), and a soft warm halo sits behind the whole mark
 * while it plays, so even when the details are too small to name, the mark is visibly lit.
 */

export const SAFFRON = '#ff9933'
export const GOLD = '#f2c14e'
export const MAGENTA = '#d63384'
export const TEAL = '#16a39a'
export const MARIGOLD = '#ffc22e'
const FLAME_RED = '#e8452c'
const FLAME_TIP = '#ffd166'
const CORE = '#fff4c2'
const CLAY = '#b85a2a'
const CLAY_DARK = '#7c3a18'

/** A flame standing on (0, 0), 4.6 tall. */
const FLAME =
  'M0 -4.6 C1.1 -3.2 1.6 -2 1.5 -1.1 C1.4 -0.3 0.8 0.2 0 0.2 C-0.8 0.2 -1.4 -0.3 -1.5 -1.1 C-1.6 -2 -1.1 -3.2 0 -4.6 Z'

/** Gradients the ideas share: the glow, the flame, and a wash per column in `washes` colours. */
function Defs({ id, washes = [] }: { id: string; washes?: readonly string[] }) {
  return (
    <defs>
      <radialGradient id={`${id}-glow`}>
        <stop offset="0" stopColor={SAFFRON} stopOpacity={0.5} />
        <stop offset="1" stopColor={SAFFRON} stopOpacity={0} />
      </radialGradient>
      <linearGradient id={`${id}-flame`} x1="0" y1="1" x2="0" y2="0">
        <stop offset="0" stopColor={FLAME_RED} />
        <stop offset="0.45" stopColor={SAFFRON} />
        <stop offset="1" stopColor={FLAME_TIP} />
      </linearGradient>
      {washes.map((color, col) => (
        <linearGradient key={color} id={`${id}-wash-${col}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={color} stopOpacity={0.95} />
          <stop offset="1" stopColor={color} stopOpacity={0} />
        </linearGradient>
      ))}
    </defs>
  )
}

/** The warm halo behind the whole mark. */
function Backlight({ id, amount }: { id: string; amount: number }) {
  return <ellipse cx={8} cy={7} rx={13} ry={11} fill={`url(#${id}-glow)`} opacity={clamp01(amount)} />
}

/** A stem, with light from its dot washing down it. */
function Stem({ id, col, wash }: { id: string; col: number; wash: number }) {
  return (
    <g>
      <rect x={colX(col)} y={6} width={4} height={10} fill={INK} />
      <rect x={colX(col)} y={6} width={4} height={10} fill={`url(#${id}-wash-${col})`} opacity={clamp01(wash)} />
    </g>
  )
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* 1. Diyas: each dot becomes a clay oil lamp, lit left to right, and the three flames flicker with a warm glow.      */
/* ------------------------------------------------------------------------------------------------------------------ */

/** A diya's clay bowl, its rim centred on (0, 0) and its spout to the right. */
const BOWL = 'M-2.5 -0.2 C-2.3 1.6 -1.1 2.1 0 2.1 C1.2 2.1 2.1 1.6 2.4 0.4 L3.3 -0.4 L2.2 -0.1 Z'
const LAMPLIGHT = '#ffe2a8'

export function DiyasMark({ on, size, still }: MarkProps) {
  const live = useTimeline(on, { duration: 1.1 })
  const t = still ? 1 : live
  const idle = on && t === 1 && !still
  const clock = useClock(idle)
  const id = useId().replace(/:/g, '')

  const lamps = COLS.map((col) => ({
    shape: easeOut(seg(t, 0.04 + 0.14 * col, 0.34 + 0.14 * col)),
    lit: seg(t, 0.24 + 0.14 * col, 0.6 + 0.14 * col),
    flicker: idle ? 0.07 * Math.sin(clock * 9 + col * 2.1) + 0.04 * Math.sin(clock * 23 + col * 5) : 0,
    sway: idle ? 5 * Math.sin(clock * 4.3 + col * 1.7) : 0,
  }))
  const room = lamps.reduce((sum, lamp) => sum + lamp.lit, 0) / 3

  return (
    <MarkSvg size={size}>
      <Defs id={id} washes={[LAMPLIGHT, LAMPLIGHT, LAMPLIGHT]} />
      <Backlight id={id} amount={0.5 * room + (idle ? 0.06 * Math.sin(clock * 3) : 0)} />
      {COLS.map((col) => {
        const cx = colX(col) + 2
        const { shape, lit, flicker, sway } = lamps[col]
        const bowl = pop(cx, 2.4, shape)
        const grow = lerp(0.3, 1, backOut(lit)) * 0.9
        const glow = easeOut(lit) * (0.85 + (idle ? 0.15 * Math.sin(clock * 11 + col) : 0))
        return (
          <g key={col}>
            <circle cx={cx + 0.3} cy={0.4} r={5.2} fill={`url(#${id}-glow)`} opacity={glow} />
            <Stem id={id} col={col} wash={0.75 * easeOut(lit)} />
            {shape < 1 && (
              <rect
                x={colX(col)}
                y={0}
                width={4}
                height={4}
                fill={INK}
                transform={scaleAt(cx, 3, 1 - 0.5 * shape)}
                opacity={1 - shape}
              />
            )}
            <g {...bowl}>
              <g transform={`translate(${cx} 2.4)`}>
                <path d={BOWL} fill={CLAY} />
                <rect x={-2.55} y={-0.55} width={5} height={0.6} rx={0.3} fill={CLAY_DARK} />
                {[-1.2, 0, 1.2].map((x) => (
                  <circle key={x} cx={x} cy={x === 0 ? 1.2 : 0.95} r={0.24} fill={GOLD} />
                ))}
              </g>
            </g>
            <g
              transform={`translate(${cx + 0.3} 1.9) rotate(${sway}) scale(${grow} ${grow * (1 + flicker)})`}
              opacity={clamp01(lit * 3)}
            >
              <path d={FLAME} fill={`url(#${id}-flame)`} />
              <path d={FLAME} fill={CORE} transform="translate(0 -0.15) scale(0.5)" />
            </g>
          </g>
        )
      })}
    </MarkSvg>
  )
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* 2. Rangoli: each dot turns into a diamond and a ring of petals blooms around it in rangoli colours, slowly turning. */
/* ------------------------------------------------------------------------------------------------------------------ */

const RANGOLI_COLORS: readonly (readonly [string, string])[] = [
  [MAGENTA, SAFFRON],
  [TEAL, GOLD],
  [SAFFRON, MAGENTA],
]
const PETALS = [0, 1, 2, 3, 4, 5, 6, 7]

export function RangoliMark({ on, size, still }: MarkProps) {
  const live = useTimeline(on, { duration: 1.2 })
  const t = still ? 1 : live
  const idle = on && t === 1 && !still
  const clock = useClock(idle)
  const id = useId().replace(/:/g, '')

  return (
    <MarkSvg size={size}>
      <Defs id={id} washes={RANGOLI_COLORS.map(([a]) => a)} />
      <Backlight id={id} amount={0.3 * easeOut(t)} />
      {COLS.map((col) => {
        const offset = 0.08 * col
        return <Stem key={col} id={id} col={col} wash={0.7 * easeOut(seg(t, 0.2 + offset, 0.6 + offset))} />
      })}
      {COLS.map((col) => {
        const cx = colX(col) + 2
        const offset = 0.08 * col
        const turn = easeInOut(seg(t, offset, 0.4 + offset))
        const spin = idle ? (col % 2 ? -1 : 1) * clock * 16 : 0
        const [a, b] = RANGOLI_COLORS[col]
        const dots = seg(t, 0.62, 0.86)
        return (
          <g key={col} transform={`translate(${cx} 2)`}>
            <g transform={`rotate(${spin})`}>
              {PETALS.map((i) => {
                const q = seg(t, 0.14 + offset + i * 0.03, 0.44 + offset + i * 0.03)
                return (
                  <g key={i} transform={`rotate(${i * 45})`} opacity={clamp01(q * 3)}>
                    <ellipse cx={0} cy={-1.9 * lerp(0.5, 1, backOut(q))} rx={0.66} ry={0.95} fill={i % 2 ? b : a} />
                  </g>
                )
              })}
              {PETALS.map((i) => (
                <circle
                  key={i}
                  cx={2.8 * Math.sin(((i * 45 + 22.5) * Math.PI) / 180)}
                  cy={-2.8 * Math.cos(((i * 45 + 22.5) * Math.PI) / 180)}
                  r={0.22}
                  fill={INK}
                  opacity={dots}
                />
              ))}
            </g>
            <rect
              x={-2}
              y={-2}
              width={4}
              height={4}
              fill={mix(INK, GOLD, turn)}
              transform={`rotate(${45 * turn}) scale(${lerp(1, 0.55, turn)})`}
            />
          </g>
        )
      })}
    </MarkSvg>
  )
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* 3. Phuljhari: the dots glow gold and throw sparks like sparklers for as long as the mark is held.                  */
/* ------------------------------------------------------------------------------------------------------------------ */

const SPARKS = Array.from({ length: 16 }, (_, k) => k)

export function SparklerMark({ on, size, still }: MarkProps) {
  const live = useTimeline(on, { duration: 0.6 })
  const t = still ? 1 : live
  // Sparks fly from the moment it turns on, not only once settled.
  const running = useClock(on && !still)
  const clock = still ? 0.37 : running
  const glow = easeOut(t)
  const id = useId().replace(/:/g, '')

  return (
    <MarkSvg size={size}>
      <Defs id={id} washes={[GOLD, GOLD, GOLD]} />
      <Backlight id={id} amount={glow * (0.5 + (clock > 0 ? 0.08 * Math.sin(clock * 17) : 0))} />
      {COLS.map((col) => (
        <Stem key={col} id={id} col={col} wash={0.8 * glow} />
      ))}
      {COLS.map((col) => {
        const cx = colX(col) + 2
        return (
          <g key={col}>
            <circle cx={cx} cy={2} r={4.2} fill={`url(#${id}-glow)`} opacity={glow} />
            {(clock > 0 || still) &&
              SPARKS.map((k) => {
                const angle = ((k / SPARKS.length) * 360 + col * 17 + ((k * 53) % 23)) * (Math.PI / 180)
                const period = 0.45 + (k % 4) * 0.1
                const phase = ((((clock + k * 0.137 + col * 0.31) % period) + period) % period) / period
                const reach = 1.8 + (k % 3) * 0.5
                const r0 = 2 + phase * reach * 1.6
                const length = 1.5 * (1 - phase * 0.6)
                const [dx, dy] = [Math.sin(angle), -Math.cos(angle)]
                const [tx, ty] = [cx + dx * (r0 + length), 2 + dy * (r0 + length)]
                // Near the end of its flight a spark splits into a small star: the crackle.
                const crackle = phase > 0.6 ? (phase - 0.6) / 0.4 : 0
                return (
                  <g key={k} opacity={(1 - phase) * glow} stroke={mix(CORE, SAFFRON, phase)} strokeLinecap="round">
                    <path d={`M${cx + dx * r0} ${2 + dy * r0} L${tx} ${ty}`} strokeWidth={0.28} />
                    {crackle > 0 && (
                      <path
                        d={`M${tx - 0.5 * crackle} ${ty} L${tx + 0.5 * crackle} ${ty} M${tx} ${ty - 0.5 * crackle} L${tx} ${ty + 0.5 * crackle}`}
                        strokeWidth={0.18}
                      />
                    )}
                  </g>
                )
              })}
            <rect x={colX(col)} y={0} width={4} height={4} fill={mix(INK, GOLD, glow)} />
            <rect x={cx - 0.9} y={1.1} width={1.8} height={1.8} fill={CORE} opacity={glow * 0.9} />
          </g>
        )
      })}
    </MarkSvg>
  )
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* 4. Fireworks: each dot is a launch pad. A rocket streaks up from it and bursts above the mark; the dot glows as    */
/*    its fuse and the stem lights with the launch. The mark itself never comes apart. Plays once.                     */
/* ------------------------------------------------------------------------------------------------------------------ */

const BURST_COLORS: readonly (readonly [string, string])[] = [
  [MAGENTA, GOLD],
  [TEAL, CORE],
  [SAFFRON, MAGENTA],
]
const BURST = Array.from({ length: 14 }, (_, i) => i)
const BURST_Y = -4.6

/** A shell bursting at (cx, BURST_Y): sparks streak outward, droop as they fall, and fade. */
function Burst({ cx, b, colors }: { cx: number; b: number; colors: readonly [string, string] }) {
  const r = 3.9 * easeOut(b)
  const tail = 1.6 * (1 - b)
  const droop = 1.6 * b * b
  return (
    <g opacity={1 - easeIn(b)} strokeLinecap="round">
      <circle cx={cx} cy={BURST_Y} r={1.6 * (1 - b)} fill={CORE} opacity={1 - b} />
      {BURST.map((i) => {
        const angle = ((i * 360) / BURST.length) * (Math.PI / 180)
        const [dx, dy] = [Math.sin(angle), -Math.cos(angle)]
        const [x, y] = [cx + dx * r, BURST_Y + dy * r + droop]
        const back = Math.max(0, r - tail)
        return (
          <g key={i}>
            <path
              d={`M${cx + dx * back} ${BURST_Y + dy * back + droop * 0.6} L${x} ${y}`}
              stroke={colors[i % 2]}
              strokeWidth={0.32}
              opacity={0.75}
            />
            <circle cx={x} cy={y} r={0.38 * (1 - 0.4 * b)} fill={colors[i % 2]} />
          </g>
        )
      })}
    </g>
  )
}

export function FireworksMark({ on, size, still }: MarkProps) {
  const t = useTimeline(on, { duration: 2.6, oneshot: true })
  const id = useId().replace(/:/g, '')

  const shots = COLS.map((col) => {
    const start = 0.04 + 0.16 * col
    return {
      fuse: still ? 1 : seg(t, start, start + 0.1),
      launch: still ? 1 : seg(t, start + 0.08, start + 0.26),
      burst: still ? 0.4 : seg(t, start + 0.24, start + 0.64),
      cool: still ? 0 : seg(t, start + 0.5, start + 0.8),
    }
  })
  const flash = Math.max(...shots.map(({ burst }) => (burst > 0 && burst < 1 ? 1 - burst : 0)))

  return (
    <MarkSvg size={size}>
      <Defs id={id} washes={[GOLD, GOLD, GOLD]} />
      <Backlight id={id} amount={0.55 * flash} />
      {COLS.map((col) => {
        const cx = colX(col) + 2
        const { fuse, launch, burst, cool } = shots[col]
        // The dot glows gold while its rocket is lit, then cools back to ink.
        const hot = easeOut(fuse) * (1 - easeOut(cool))
        const rising = launch > 0 && launch < 1
        const head = lerp(0.5, BURST_Y + 0.6, easeOut(launch))
        const tail = Math.min(0.5, head + 3.2 * (1 - launch))
        return (
          <g key={col}>
            <Stem id={id} col={col} wash={0.8 * hot} />
            <rect x={colX(col)} y={0} width={4} height={4} fill={mix(INK, GOLD, hot)} />
            {hot > 0 && <rect x={cx - 0.9} y={1.1} width={1.8} height={1.8} fill={CORE} opacity={hot * 0.9} />}
            {rising && (
              <path
                d={`M${cx} ${tail} L${cx} ${head}`}
                stroke={GOLD}
                strokeWidth={0.5}
                strokeLinecap="round"
                opacity={0.9 * (1 - launch * 0.5)}
              />
            )}
            {burst > 0 && burst < 1 && <Burst cx={cx} b={burst} colors={BURST_COLORS[col]} />}
          </g>
        )
      })}
    </MarkSvg>
  )
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* 5. Marigold toran: a garland of marigolds strings itself across the top of the mark and sways gently.             */
/* ------------------------------------------------------------------------------------------------------------------ */

type Point = readonly [number, number]
const STRING = '#8a5a2b'
const LEAF = '#3f8f3a'
const MARIGOLD_DEEP = '#e07b00'

/** The garland's swags, as quadratic curves: hanging off the left, over each dot, and off the right. */
function swags(sag: number): readonly (readonly [Point, Point, Point])[] {
  return [
    [
      [-1.4, 3.8],
      [-0.8, 0.2],
      [2, -0.5],
    ],
    [
      [2, -0.5],
      [5, 6.2 + sag],
      [8, -0.5],
    ],
    [
      [8, -0.5],
      [11, 6.2 - sag],
      [14, -0.5],
    ],
    [
      [14, -0.5],
      [16.8, 0.2],
      [17.4, 3.8],
    ],
  ]
}

const at = ([a, c, b]: readonly [Point, Point, Point], u: number): Point => [
  (1 - u) ** 2 * a[0] + 2 * (1 - u) * u * c[0] + u * u * b[0],
  (1 - u) ** 2 * a[1] + 2 * (1 - u) * u * c[1] + u * u * b[1],
]

function Marigold({ x, y, n }: { x: number; y: number; n: number }) {
  return (
    <g>
      <circle cx={x} cy={y} r={1.15} fill={n % 2 ? MARIGOLD : SAFFRON} />
      <circle cx={x} cy={y} r={0.62} fill={MARIGOLD_DEEP} />
      <circle cx={x} cy={y} r={0.22} fill="#7a3d00" />
    </g>
  )
}

export function ToranMark({ on, size, still }: MarkProps) {
  const live = useTimeline(on, { duration: 1.25 })
  const t = still ? 1 : live
  const idle = on && t === 1 && !still
  const clock = useClock(idle)
  const id = useId().replace(/:/g, '')
  const curves = swags(idle ? 0.45 * Math.sin(clock * 2) : 0)

  const flowers = curves.flatMap((curve, k) =>
    [0, 0.25, 0.5, 0.75, ...(k === curves.length - 1 ? [1] : [])].map((u) => at(curve, u)),
  )
  const tassels: Point[] = [
    [-1.4, 5.5],
    [-1.4, 7.1],
    [17.4, 5.5],
    [17.4, 7.1],
  ]
  const thread = easeOut(seg(t, 0, 0.45))
  const leaves = seg(t, 0.55, 0.8)

  return (
    <MarkSvg size={size}>
      <Defs id={id} washes={[MARIGOLD, MARIGOLD, MARIGOLD]} />
      <Backlight id={id} amount={0.3 * easeOut(t)} />
      {COLS.map((col) => (
        <g key={col}>
          <Stem id={id} col={col} wash={0.45 * easeOut(seg(t, 0.3, 0.8))} />
          <rect x={colX(col)} y={0} width={4} height={4} fill={INK} />
        </g>
      ))}
      <path
        d={curves.map(([a, c, b], i) => `${i ? '' : `M${a[0]} ${a[1]} `}Q${c[0]} ${c[1]} ${b[0]} ${b[1]}`).join(' ')}
        fill="none"
        stroke={STRING}
        strokeWidth={0.2}
        pathLength={1}
        strokeDasharray={1}
        strokeDashoffset={1 - thread}
      />
      <path d="M-1.4 3.8 L-1.4 7.1 M17.4 3.8 L17.4 7.1" stroke={STRING} strokeWidth={0.2} opacity={seg(t, 0.4, 0.6)} />
      {[2, 8, 14].map((x) => (
        <g key={x} opacity={leaves}>
          <ellipse cx={x - 0.9} cy={-1.5} rx={0.46} ry={1.1} fill={LEAF} transform={`rotate(-40 ${x - 0.9} -1.5)`} />
          <ellipse cx={x + 0.9} cy={-1.5} rx={0.46} ry={1.1} fill={LEAF} transform={`rotate(40 ${x + 0.9} -1.5)`} />
        </g>
      ))}
      {[...flowers, ...tassels].map(([x, y], i) => {
        const q = seg(t, 0.08 + i * 0.035, 0.3 + i * 0.035)
        return (
          // biome-ignore lint/suspicious/noArrayIndexKey: a fixed string of flowers, never reordered
          <g key={i} {...pop(x, y, q)}>
            <Marigold x={x} y={y} n={i} />
          </g>
        )
      })}
    </MarkSvg>
  )
}
