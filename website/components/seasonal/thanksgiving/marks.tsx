'use client'

import {
  backOut,
  blink,
  clamp01,
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

/* Thanksgiving takes on the iii mark. At t = 0 every idea draws exactly the plain mark. */

export const RED = '#c9472f'
export const ORANGE = '#e5852e'
export const AMBER = '#ebb24b'
export const BODY = '#7c4c2c'
export const BODY_DARK = '#5f3820'
const BREAST = '#94603a'
const HEAD = '#9a643b'
export const TAN = '#c08048'
export const WATTLE = '#e04a3c'
export const BEAK = '#f4b53c'
export const PUPIL = '#1d130c'
export const BAND = '#3b2014'
/** Feather tips blend toward this gold; blending red toward white reads pink. */
export const TIP = '#f6d48c'

/* ------------------------------------------------------------------------------------------------------------------ */
/* 1. Strut: the mark becomes the turkey. Stems fan out into tail feathers, the dots become body, head and wattle.    */
/* ------------------------------------------------------------------------------------------------------------------ */

const PIVOT = { x: 8, y: 13 }
/** The three stems become the front feathers. Every feather is the same length, so the tips draw one arc. */
const STRUT_STEMS = [
  { col: 0, angle: -51, fill: ORANGE, length: 13, width: 4.2 },
  { col: 1, angle: 0, fill: RED, length: 13, width: 4.2 },
  { col: 2, angle: 51, fill: ORANGE, length: 13, width: 4.2 },
]
/** Four more grow in behind them, the inner pair first, so the fan opens outward. */
const STRUT_EXTRAS = [
  { angle: -26, fill: AMBER, at: 0.3 },
  { angle: 26, fill: AMBER, at: 0.3 },
  { angle: -76, fill: RED, at: 0.37 },
  { angle: 76, fill: RED, at: 0.37 },
]

/** A tail feather standing on (x, y): a rounded tip in a lighter shade of its colour, edged by a thin dark band. */
function Feather({
  x,
  y,
  angle,
  w,
  h,
  r,
  fill,
  tipFill,
  tip,
  opacity = 1,
}: {
  x: number
  y: number
  angle: number
  w: number
  h: number
  r: number
  fill: string
  tipFill: string
  tip: number
  opacity?: number
}) {
  const cap = Math.min(2.6, h)
  return (
    <g transform={`translate(${x} ${y}) rotate(${angle})`} opacity={opacity}>
      <rect x={-w / 2} y={-h} width={w} height={h} rx={r} fill={fill} />
      <rect x={-w / 2} y={-h} width={w} height={cap} rx={r} fill={tipFill} opacity={tip} />
      <rect x={-w / 2} y={-h + cap} width={w} height={0.55} fill={BAND} opacity={0.45 * tip} />
    </g>
  )
}

export function StrutMark({ on, size, still }: MarkProps) {
  const live = useTimeline(on, { duration: 1.15 })
  const t = still ? 1 : live
  const idle = on && t === 1 && !still
  const clock = useClock(idle)

  const fan = easeInOut(seg(t, 0, 0.5))
  const tint = easeOut(seg(t, 0, 0.3))
  const fold = easeInOut(seg(t, 0.1, 0.55))
  const tip = easeOut(seg(t, 0.62, 0.88))
  const breast = easeOut(seg(t, 0.45, 0.7))

  // Idle life while held: the fan breathes, and every few seconds the head gives a little gobble.
  const breathe = idle ? 1 + 0.035 * Math.sin(clock * 2.2) : 1
  const gobblePhase = clock % 3.4
  const gobble =
    idle && gobblePhase > 1.6 && gobblePhase < 2.15
      ? Math.sin(((gobblePhase - 1.6) / 0.55) * Math.PI * 4) * (1 - (gobblePhase - 1.6) / 0.55)
      : 0
  const headShift = 0.35 * gobble
  const feet = pop(8, 14.6, seg(t, 0.58, 0.8))
  const beak = pop(8 + headShift, 6.3, seg(t, 0.55, 0.76))
  const eyes = pop(8 + headShift, 4.9, seg(t, 0.62, 0.84), blink(clock))

  return (
    <MarkSvg size={size}>
      {STRUT_EXTRAS.map((f) => {
        const p = seg(t, f.at, f.at + 0.32)
        return (
          <Feather
            key={f.angle}
            x={PIVOT.x}
            y={PIVOT.y}
            angle={f.angle * breathe}
            w={4.2}
            h={13 * lerp(0.35, 1, backOut(p))}
            r={2.1}
            fill={f.fill}
            tipFill={mix(f.fill, TIP, 0.5)}
            tip={tip}
            opacity={clamp01(p * 3)}
          />
        )
      })}
      {STRUT_STEMS.map((s) => (
        <Feather
          key={s.col}
          x={lerp(colX(s.col) + 2, PIVOT.x, fan)}
          y={lerp(16, PIVOT.y, fan)}
          angle={s.angle * fan * breathe}
          w={lerp(4, s.width, fan)}
          h={lerp(10, s.length, fan)}
          r={(s.width / 2) * fan}
          fill={mix(INK, s.fill, tint)}
          tipFill={mix(s.fill, TIP, 0.5)}
          tip={tip}
        />
      ))}

      <g transform={feet.transform} opacity={feet.opacity} fill={BEAK}>
        <rect x={6} y={14.6} width={0.8} height={2} />
        <rect x={9.2} y={14.6} width={0.8} height={2} />
        <rect x={5.4} y={16.2} width={2} height={0.6} rx={0.3} />
        <rect x={8.6} y={16.2} width={2} height={0.6} rx={0.3} />
      </g>

      {/* Dot 1 → body, dot 2 → head, dot 3 → wattle. */}
      <rect
        x={lerp(0, 3.3, fold)}
        y={lerp(0, 8, fold)}
        width={lerp(4, 9.4, fold)}
        height={lerp(4, 7.6, fold)}
        rx={3.8 * fold}
        fill={mix(INK, BODY, tint)}
      />
      <g opacity={breast}>
        <rect x={3.7} y={10.4} width={2.1} height={3.8} rx={1.05} fill={BODY_DARK} />
        <rect x={10.2} y={10.4} width={2.1} height={3.8} rx={1.05} fill={BODY_DARK} />
        <rect x={5.6} y={9.8} width={4.8} height={4.6} rx={2.3} fill={BREAST} />
      </g>
      <g transform={`translate(${headShift} 0)`}>
        <rect
          x={lerp(6, 5.9, fold)}
          y={lerp(0, 3.1, fold)}
          width={lerp(4, 4.2, fold)}
          height={lerp(4, 5.7, fold)}
          rx={2.1 * fold}
          fill={mix(INK, HEAD, tint)}
        />
        <rect
          x={lerp(12, 8.35, fold)}
          y={lerp(0, 6.3, fold)}
          width={lerp(4, 1.3, fold)}
          height={lerp(4, 3.1, fold)}
          rx={0.65 * fold}
          fill={mix(INK, WATTLE, tint)}
          transform={`rotate(${14 * gobble} 9 6.3)`}
        />
        <path d="M7.15 5.95 L8.85 5.95 L8 7.25 Z" fill={BEAK} transform={beak.transform} opacity={beak.opacity} />
        <g transform={eyes.transform} opacity={eyes.opacity}>
          <circle cx={7.05} cy={4.9} r={0.62} fill={INK} />
          <circle cx={8.95} cy={4.9} r={0.62} fill={INK} />
          <circle cx={7.15} cy={5} r={0.34} fill={PUPIL} />
          <circle cx={9.05} cy={5} r={0.34} fill={PUPIL} />
        </g>
      </g>
    </MarkSvg>
  )
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* 2. Gobble: the mark stays the mark. The middle dot becomes a small turkey head that pecks now and then.            */
/* ------------------------------------------------------------------------------------------------------------------ */

const GOBBLE_PERIOD = 1.9
const PECK = 0.42

export function GobbleMark({ on, size, still }: MarkProps) {
  const live = useTimeline(on, { duration: 0.9 })
  const t = still ? 1 : live
  const idle = on && t === 1 && !still
  const clock = useClock(idle)

  const tint = easeOut(seg(t, 0, 0.45))
  const wattle = easeOut(seg(t, 0.45, 0.8))
  const comb = pop(7.35, 0.4, seg(t, 0.3, 0.6))
  const beak = pop(10, 2.15, seg(t, 0.35, 0.6))
  const eye = pop(8.6, 1.5, seg(t, 0.5, 0.75), blink(clock, 2.3))

  const phase = clock % GOBBLE_PERIOD
  const peck = clock > 0 && phase < PECK ? Math.sin((phase / PECK) * Math.PI) : 0
  const hop = clock > 0 && phase > 0.08 && phase < PECK + 0.08 ? Math.sin(((phase - 0.08) / PECK) * Math.PI) : 0

  return (
    <MarkSvg size={size}>
      {COLS.map((col) => (
        <rect key={col} x={colX(col)} y={6} width={4} height={10} fill={INK} />
      ))}
      <rect x={0} y={-1.1 * hop} width={4} height={4} fill={mix(INK, ORANGE, tint)} />
      <rect x={12} y={-1.1 * hop} width={4} height={4} fill={mix(INK, AMBER, tint)} />

      <g transform={`rotate(${18 * peck} 8 4)`}>
        <rect x={6.5} y={-1.7} width={1.7} height={2.1} rx={0.85} fill={WATTLE} {...comb} />
        <rect x={6} y={0} width={4} height={4} rx={0.8 * tint} fill={mix(INK, TAN, tint)} />
        <rect x={10.05} y={2.6} width={1.05} height={3.2 * wattle} rx={0.52} fill={WATTLE} />
        <path d="M10 1.3 L12.1 2.15 L10 3 Z" fill={BEAK} {...beak} />
        <g {...eye}>
          <circle cx={8.6} cy={1.5} r={0.7} fill={INK} />
          <circle cx={8.78} cy={1.55} r={0.38} fill={PUPIL} />
        </g>
      </g>
    </MarkSvg>
  )
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* 3. Turkey trot: a tiny turkey walks across the tops of the dots, which dip under its feet. Plays once.             */
/* ------------------------------------------------------------------------------------------------------------------ */

/** A side-view turkey facing right, standing with its feet at (0, 0). `step` drives the legs. */
function SideTurkey({ step }: { step: number }) {
  const swing = 0.55 * Math.sin(step)
  return (
    <g>
      <g stroke={BEAK} strokeWidth={0.36} strokeLinecap="round">
        <path d={`M-0.3 -1.1 L${-0.3 + swing} 0`} />
        <path d={`M0.35 -1.1 L${0.35 - swing} 0`} />
      </g>
      {[
        [-72, RED],
        [-42, ORANGE],
        [-12, AMBER],
      ].map(([angle, fill]) => (
        <ellipse
          key={angle}
          cx={0}
          cy={-1.8}
          rx={0.85}
          ry={1.9}
          fill={fill as string}
          transform={`translate(-1.1 -2.3) rotate(${angle})`}
        />
      ))}
      <ellipse cx={0} cy={-2.2} rx={1.9} ry={1.35} fill={BODY} />
      <ellipse cx={-0.35} cy={-2.25} rx={1} ry={0.7} fill={BODY_DARK} />
      <rect x={0.95} y={-4.6} width={1.1} height={2.4} rx={0.55} fill={TAN} />
      <circle cx={1.75} cy={-4.6} r={0.85} fill={TAN} />
      <rect x={2.3} y={-4.35} width={0.45} height={1.15} rx={0.22} fill={WATTLE} />
      <path d="M2.45 -4.85 L3.4 -4.5 L2.45 -4.15 Z" fill={BEAK} />
      <circle cx={1.95} cy={-4.85} r={0.22} fill={PUPIL} />
    </g>
  )
}

const GAPS = [5, 11]

export function TrotMark({ on, size, still }: MarkProps) {
  const live = useTimeline(on, { duration: 2.8, oneshot: true })
  const t = live
  const walking = !still && t > 0

  const x = still ? 8 : lerp(-7, 23, t)
  const step = still ? 0 : t * 30 * Math.PI
  const bob = walking ? -0.3 * Math.abs(Math.sin(step)) : 0
  // It hops the gaps between the dots rather than walking through them.
  const hop = Math.max(0, ...GAPS.map((g) => 1 - ((x - g) / 2.3) ** 2)) * 1.5
  const dip = (col: number) => (walking || still ? Math.max(0, 1 - Math.abs(x - (colX(col) + 2)) / 2.6) : 0)
  const under = Math.max(...COLS.map(dip))
  const opacity = still ? 1 : seg(x, -7, -3.5) * (1 - seg(x, 19.5, 23))

  return (
    <MarkSvg size={size}>
      {COLS.map((col) => (
        <g key={col}>
          <rect x={colX(col)} y={6} width={4} height={10} fill={INK} />
          <rect
            x={colX(col)}
            y={0.55 * dip(col)}
            width={4}
            height={4}
            fill={INK}
            transform={scaleAt(colX(col) + 2, 4, 1, 1 - 0.07 * dip(col))}
          />
        </g>
      ))}
      {(walking || still) && (
        <g transform={`translate(${x} ${0.55 * under - hop + bob}) scale(0.9)`} opacity={opacity}>
          <SideTurkey step={step} />
        </g>
      )}
    </MarkSvg>
  )
}

/* ------------------------------------------------------------------------------------------------------------------ */
/* 4. Gobble chorus: all three dots become turkey heads, pecking one after another like a round.                      */
/* ------------------------------------------------------------------------------------------------------------------ */

const SKINS = ['#a9683a', TAN, '#b5743f']
const CHORUS_PERIOD = 2.1
const CHORUS_PECK = 0.42

function TurkeyHead({ col, t, clock }: { col: number; t: number; clock: number }) {
  const offset = 0.1 * col
  const tint = easeOut(seg(t, offset, 0.4 + offset))
  const wattle = easeOut(seg(t, 0.45 + offset, 0.8 + offset))
  const comb = pop(7.35, 0.4, seg(t, 0.3 + offset, 0.6 + offset))
  const beak = pop(10, 2.15, seg(t, 0.35 + offset, 0.6 + offset))
  const eye = pop(8.6, 1.5, seg(t, 0.5 + offset, 0.75 + offset), blink(clock - col * 0.4, 2.5))

  const local = clock - col * 0.2
  const phase = ((local % CHORUS_PERIOD) + CHORUS_PERIOD) % CHORUS_PERIOD
  const peck = clock > 0 && local > 0 && phase < CHORUS_PECK ? Math.sin((phase / CHORUS_PECK) * Math.PI) : 0

  return (
    <g transform={`translate(${colX(col) - 6} 0) rotate(${18 * peck} 8 4)`}>
      <rect x={6.5} y={-1.7} width={1.7} height={2.1} rx={0.85} fill={WATTLE} {...comb} />
      <rect x={6} y={0} width={4} height={4} rx={0.8 * tint} fill={mix(INK, SKINS[col], tint)} />
      <rect x={10.05} y={2.6} width={1.05} height={3.2 * wattle} rx={0.52} fill={WATTLE} />
      <path d="M10 1.3 L12.1 2.15 L10 3 Z" fill={BEAK} {...beak} />
      <g {...eye}>
        <circle cx={8.6} cy={1.5} r={0.7} fill={INK} />
        <circle cx={8.78} cy={1.55} r={0.38} fill={PUPIL} />
      </g>
    </g>
  )
}

export function ChorusMark({ on, size, still }: MarkProps) {
  const live = useTimeline(on, { duration: 1.1 })
  const t = still ? 1 : live
  const idle = on && t === 1 && !still
  const clock = useClock(idle)

  return (
    <MarkSvg size={size}>
      {COLS.map((col) => (
        <rect key={col} x={colX(col)} y={6} width={4} height={10} fill={INK} />
      ))}
      {COLS.map((col) => (
        <TurkeyHead key={col} col={col} t={t} clock={clock} />
      ))}
    </MarkSvg>
  )
}
