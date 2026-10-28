'use client'

import { ArrowUpRightIcon, PlusIcon } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useState } from 'react'

import { CanvasRevealEffect } from '@/components/ui/canvas-reveal-effect'
import { useMotionPreference } from '@/hooks/use-motion-preference'
import type { Contributor } from '@/lib/contributors'
import { duration, easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

/** Avatar size drawn, and the 2x GitHub renders for it (`s=` is GitHub's avatar size parameter). */
const AVATAR_PX = 40
const AVATAR_SRC_PX = AVATAR_PX * 2

/** The reveal's dots at a fraction of the stock brightness: a texture behind the name, not a competitor to it. */
const DOT_OPACITIES = [0.08, 0.08, 0.08, 0.14, 0.14, 0.14, 0.22, 0.22, 0.22, 0.6]

/** Every cell is the same fixed height, so the grid's shape never depends on a name's length. */
const cell =
  'group relative flex h-[108px] w-full flex-col items-center justify-center overflow-hidden px-2 outline-none focus-visible:z-10 focus-visible:outline-2 focus-visible:outline-foreground focus-visible:-outline-offset-2'

const avatar =
  'relative z-10 size-10 shrink-0 overflow-hidden rounded-full bg-muted shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--foreground)_12%,transparent)]'

const name =
  'relative z-10 mt-2.5 w-full truncate text-center font-sans text-[13px] leading-none transition-colors duration-150'

/** Hover is a mouse thing. On a touch screen a tap would set it and leave it set, so only hover-capable devices get it. */
const canHover = () => window.matchMedia('(hover: hover)').matches

/**
 * One cell per person on a hairline grid, like a table, the avatar in grayscale with the login beneath it. Hover
 * (or keyboard focus) is where the colour is: the white dot-matrix reveal animates in on black, the avatar turns
 * to colour, the name turns white, the commit count fades in under it and an arrow slides into the corner the way
 * the footer's external links do. The whole cell is the link to the profile. The canvas mounts only while a cell
 * is hovered, so sixty cells cost at most one WebGL context; reduced motion gets a plain black fill.
 */
function Tile({ contributor: c }: { contributor: Contributor }) {
  const [hovered, setHovered] = useState(false)
  const reduce = useMotionPreference()
  const commits = `${c.contributions.toLocaleString('en-US')} ${c.contributions === 1 ? 'commit' : 'commits'}`
  return (
    <li className="min-w-0 border-r border-b">
      <a
        href={c.url}
        target="_blank"
        rel="noopener noreferrer"
        aria-label={`${c.login}, ${commits}, on GitHub`}
        data-hovered={hovered}
        onMouseEnter={() => canHover() && setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        onFocus={() => setHovered(true)}
        onBlur={() => setHovered(false)}
        className={cell}
      >
        <AnimatePresence>
          {hovered ? (
            <motion.div
              aria-hidden
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: duration.base, ease: easeOut }}
              className="pointer-events-none absolute inset-0 bg-black"
            >
              {reduce ? null : (
                <CanvasRevealEffect
                  animationSpeed={4}
                  containerClassName="bg-black"
                  colors={[[255, 255, 255]]}
                  opacities={DOT_OPACITIES}
                  dotSize={2}
                  showGradient={false}
                />
              )}
              {/* Calm the dots behind the avatar and the name, so the text reads at a glance. */}
              <div className="absolute inset-0 bg-black/80 [mask-image:radial-gradient(60%_60%_at_center,black,transparent)]" />
            </motion.div>
          ) : null}
        </AnimatePresence>

        {/* The corner arrow: the site's external-link affordance, sliding in from the inside. */}
        <ArrowUpRightIcon
          aria-hidden
          strokeWidth={1.75}
          className="absolute top-2 right-2 z-10 size-3.5 translate-x-1 translate-y-1 text-white opacity-0 transition-[opacity,translate] duration-150 ease-out group-data-[hovered=true]:translate-x-0 group-data-[hovered=true]:translate-y-0 group-data-[hovered=true]:opacity-100 motion-reduce:transition-none"
        />

        <span
          className={cn(
            avatar,
            'grayscale transition-[filter,box-shadow] duration-300 ease-out motion-reduce:transition-none',
            'group-data-[hovered=true]:grayscale-0 group-data-[hovered=true]:shadow-[0_0_0_1px_white]',
          )}
        >
          <img
            src={`${c.avatarUrl}&s=${AVATAR_SRC_PX}`}
            width={AVATAR_PX}
            height={AVATAR_PX}
            alt=""
            loading="lazy"
            decoding="async"
            className="size-full object-cover"
          />
        </span>

        {/* The name, and the commit count in the space reserved under it, so nothing shifts on hover. */}
        <span className={cn(name, 'text-muted-foreground group-data-[hovered=true]:text-white')}>{c.login}</span>
        <span
          aria-hidden
          className="relative z-10 mt-1.5 h-[11px] translate-y-1 truncate font-mono text-[11px] text-white/60 leading-none opacity-0 transition-[opacity,translate] duration-150 ease-out group-data-[hovered=true]:translate-y-0 group-data-[hovered=true]:opacity-100 motion-reduce:transition-none"
        >
          {commits}
        </span>
      </a>
    </li>
  )
}

/** The last cell: where the reader's own name would go. A dashed ring with a plus, pointing at the contributing guide. */
function JoinTile({ label, href }: { label: string; href: string }) {
  return (
    <li className="min-w-0 border-r border-b">
      <a href={href} target="_blank" rel="noopener noreferrer" className={cn(cell, 'hover:bg-faint')}>
        <span
          aria-hidden
          className="relative z-10 flex size-10 shrink-0 items-center justify-center rounded-full border border-line-strong border-dashed text-muted-foreground transition-colors duration-150 group-hover:border-foreground group-hover:text-foreground"
        >
          <PlusIcon strokeWidth={1.75} className="size-4" />
        </span>
        <span className={cn(name, 'text-muted-foreground group-hover:text-foreground')}>{label}</span>
        <span aria-hidden className="mt-1.5 h-[11px]" />
      </a>
    </li>
  )
}

export function ContributorGrid({
  contributors,
  join,
}: {
  contributors: Contributor[]
  join: { label: string; href: string }
}) {
  return (
    <ul className="grid grid-cols-3 border-t border-l sm:grid-cols-4 md:grid-cols-6 lg:grid-cols-8 xl:grid-cols-10">
      {contributors.map((c) => (
        <Tile key={c.id} contributor={c} />
      ))}
      <JoinTile {...join} />
    </ul>
  )
}
