'use client'

import { motion, type Transition } from 'motion/react'
import { useRef, useState } from 'react'

import { cn } from '@/lib/utils'

import { MONSTERS, type Px, tint } from './monsters'

/*
 * The iii mark, haunted. When it turns on, the six bars fold into a square while it blushes toward the monster's
 * colour, then the square turns like the front of a cube to show the monster's face on the next side. Turning off
 * plays it backwards.
 *
 * The bars use the mark's own proportions on a 16-unit grid: bars 4 wide with 2-unit gaps, a dot 4 tall, a gap of
 * 2, a stem 10 tall. Folded, each column takes a third of the square and dot and stem meet halfway down.
 */
const BARS = [0, 1, 2].flatMap((col) => [
  { mark: { left: col * 6, top: 0, width: 4, height: 4 }, fold: { left: col * (16 / 3), top: 0, height: 8.1 } },
  { mark: { left: col * 6, top: 6, width: 4, height: 10 }, fold: { left: col * (16 / 3), top: 8, height: 8 } },
])

const pct = (u: number) => `${(u / 16) * 100}%`

const FOLD_IN: Transition = { duration: 0.22, ease: [0.2, 0, 0, 1] }
/** Unfolding waits for the cube to turn back, unless it never got as far as turning. */
const FOLD_OUT = (turned: boolean): Transition => ({ duration: 0.24, ease: [0.4, 0, 0.2, 1], delay: turned ? 0.3 : 0 })
const TURN_IN: Transition = { type: 'spring', stiffness: 210, damping: 19, delay: 0.16 }
const TURN_OUT: Transition = { type: 'spring', stiffness: 260, damping: 28, delay: 0.04 }

function Rects({ px, fill }: { px: readonly Px[]; fill?: string }) {
  return px.map(([x, y, width, height, f], i) => (
    // biome-ignore lint/suspicious/noArrayIndexKey: static pixel art, never reordered
    <rect key={i} x={x} y={y} width={width} height={height} fill={f ?? fill} />
  ))
}

/** The animation alone, with no interaction: whoever renders it decides when it is `on`. */
export function HauntedMark({
  on,
  monster,
  size,
  ink = '#ffffff',
  className,
}: {
  on: boolean
  /** Index into MONSTERS. A change waits until the mark is back at rest, so a face never swaps mid-turn. */
  monster: number
  /** Any CSS length; the cube depth is derived from it. */
  size: string
  /** The mark's colour at rest. */
  ink?: string
  className?: string
}) {
  const [shown, setShown] = useState(monster)
  const [resting, setResting] = useState(true)
  if (resting && shown !== monster) setShown(monster)
  if (resting && on) setResting(false)

  // Whether the cube has started to turn since it last came to rest; read when the fold-out is scheduled.
  const turned = useRef(false)

  const m = MONSTERS[shown % MONSTERS.length]
  const state = on ? 'on' : 'off'

  return (
    <div
      aria-hidden
      className={cn('relative perspective-[900px]', className)}
      style={{ width: size, height: size, ['--s' as string]: size }}
    >
      <motion.div
        className="relative size-full transform-3d [transform-origin:50%_50%_calc(var(--s)*-0.5)]"
        initial={false}
        animate={state}
        variants={{ off: { rotateY: 0, transition: TURN_OUT }, on: { rotateY: -90, transition: TURN_IN } }}
        onUpdate={(latest) => {
          if (Number(latest.rotateY) < -1) turned.current = true
        }}
        onAnimationComplete={(definition) => {
          if (definition !== 'off') return
          turned.current = false
          setResting(true)
        }}
      >
        {/* Front: the mark folding into a square. */}
        <motion.div
          className="absolute inset-0 backface-hidden"
          initial={false}
          animate={{ filter: on ? 'brightness(0.55)' : 'brightness(1)' }}
          transition={on ? TURN_IN : TURN_OUT}
        >
          {BARS.map((bar, i) => (
            <motion.span
              // biome-ignore lint/suspicious/noArrayIndexKey: the six bars are fixed
              key={i}
              className="absolute block"
              initial={false}
              animate={state}
              variants={{
                off: {
                  left: pct(bar.mark.left),
                  top: pct(bar.mark.top),
                  width: pct(bar.mark.width),
                  height: pct(bar.mark.height),
                  backgroundColor: ink,
                  transition: FOLD_OUT(turned.current),
                },
                on: {
                  left: pct(bar.fold.left),
                  top: pct(bar.fold.top),
                  width: pct(16 / 3 + 0.1),
                  height: pct(bar.fold.height),
                  backgroundColor: tint(m.color, 0.45),
                  transition: FOLD_IN,
                },
              }}
            />
          ))}
        </motion.div>

        {/* Right side: the monster, turned toward the viewer by the cube's -90° spin. */}
        <motion.div
          className="absolute top-0 left-full size-full origin-left rotate-y-90 backface-hidden"
          initial={false}
          animate={{ filter: on ? 'brightness(1)' : 'brightness(0.5)' }}
          transition={on ? TURN_IN : TURN_OUT}
        >
          <svg
            aria-hidden="true"
            viewBox="-4 -4 24 24"
            shapeRendering="crispEdges"
            className="absolute -inset-1/4 size-[150%] overflow-visible"
          >
            <motion.g
              initial={false}
              animate={state}
              variants={{
                off: { opacity: 0, scale: 0.7, transition: { duration: 0.1 } },
                on: { opacity: 1, scale: 1, transition: { type: 'spring', stiffness: 500, damping: 20, delay: 0.42 } },
              }}
              style={{ transformBox: 'view-box', transformOrigin: '50% 50%' }}
            >
              <Rects px={m.extras ?? []} />
            </motion.g>
            <Rects px={m.head} fill={m.color} />
            <Rects px={m.marks ?? []} />
            <Rects px={m.visor ?? []} />
            {m.eyes.map(([x, y, width, height, fill], i) => (
              <motion.rect
                // biome-ignore lint/suspicious/noArrayIndexKey: two eyes, fixed order
                key={i}
                x={x}
                y={y}
                width={width}
                height={height}
                fill={fill}
                style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
                animate={on ? { scaleY: [1, 1, 0.15, 1] } : { scaleY: 1 }}
                transition={
                  on ? { duration: 0.24, times: [0, 0.4, 0.6, 1], delay: 0.9, repeat: Infinity, repeatDelay: 2.2 } : {}
                }
              />
            ))}
            <Rects px={m.details ?? []} />
          </svg>
        </motion.div>
      </motion.div>
    </div>
  )
}
