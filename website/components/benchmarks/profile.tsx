'use client'

import { AnimatePresence, motion } from 'motion/react'
import { createContext, useContext, useEffect, useId, useRef, useState } from 'react'

import type { ProfileView } from '@/lib/benchmarks/views'
import { duration, easeOut, spring } from '@/lib/motion'
import { cn } from '@/lib/utils'

type ProfileState = {
  profiles: ProfileView[]
  selected: string
  select: (name: string) => void
}

const ProfileContext = createContext<ProfileState | null>(null)

/**
 * The machine size the page is looking at. One choice for the whole page: the figures at the top show it, and
 * every chart draws its line in full while the other sizes stay in the background for comparison.
 */
export function ProfileProvider({
  profiles,
  initial,
  children,
}: {
  profiles: ProfileView[]
  initial: string
  children: React.ReactNode
}) {
  const [selected, setSelected] = useState(initial)
  return (
    <ProfileContext.Provider value={{ profiles, selected, select: setSelected }}>{children}</ProfileContext.Provider>
  )
}

export function useProfile(): ProfileState {
  const state = useContext(ProfileContext)
  if (!state) throw new Error('useProfile outside ProfileProvider')
  return state
}

/** The profile tabs: a pill slides to the selected size; arrow keys move between them. */
export function ProfileTabs({ className, compact = false }: { className?: string; compact?: boolean }) {
  const { profiles, selected, select } = useProfile()
  const id = useId()
  return (
    <div
      role="tablist"
      aria-label="Machine size"
      className={cn('flex items-center gap-0.5 rounded-xl border bg-muted/60 p-1', className)}
    >
      {profiles.map((p) => {
        const on = p.name === selected
        return (
          <button
            key={p.name}
            type="button"
            role="tab"
            id={`${id}-${p.name}`}
            aria-selected={on}
            tabIndex={on ? 0 : -1}
            onClick={() => select(p.name)}
            onKeyDown={(e) => {
              if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return
              e.preventDefault()
              const i = profiles.findIndex((x) => x.name === selected)
              const next = profiles[(i + (e.key === 'ArrowRight' ? 1 : profiles.length - 1)) % profiles.length]
              select(next.name)
              document.getElementById(`${id}-${next.name}`)?.focus()
            }}
            className={cn(
              'relative flex h-8 shrink-0 items-center whitespace-nowrap rounded-lg px-3 font-sans text-[13.5px] outline-none transition-colors duration-150',
              'focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2',
              on ? 'text-foreground' : 'text-muted-foreground hover:text-foreground',
            )}
          >
            {on ? (
              <motion.span
                layoutId={`${id}-pill`}
                aria-hidden
                className="absolute inset-0 rounded-lg border bg-background shadow-[0_1px_2px_0_oklch(0_0_0/30%)]"
                transition={spring.snappy}
              />
            ) : null}
            <span className="relative tabular-nums">{compact ? p.short : p.label}</span>
          </button>
        )
      })}
    </div>
  )
}

/**
 * The same tabs, docked at the bottom of the screen once the ones in the hero have scrolled away, so a reader
 * deep in the charts can still switch sizes without scrolling back up.
 */
export function FloatingProfileTabs({ anchorId }: { anchorId: string }) {
  const [shown, setShown] = useState(false)
  const observer = useRef<IntersectionObserver | null>(null)

  useEffect(() => {
    const anchor = document.getElementById(anchorId)
    if (!anchor) return
    observer.current = new IntersectionObserver(
      ([entry]) => setShown(!entry.isIntersecting && entry.boundingClientRect.top < 0),
      {
        threshold: 0,
      },
    )
    observer.current.observe(anchor)
    return () => observer.current?.disconnect()
  }, [anchorId])

  return (
    <AnimatePresence>
      {shown ? (
        <motion.div
          initial={{ opacity: 0, transform: 'translate(-50%, 12px)' }}
          animate={{ opacity: 1, transform: 'translate(-50%, 0px)' }}
          exit={{ opacity: 0, transform: 'translate(-50%, 12px)' }}
          transition={{ duration: duration.slow, ease: easeOut }}
          className="fixed bottom-5 left-1/2 z-40 max-w-[calc(100vw-2rem)] overflow-x-auto rounded-2xl border bg-background/80 p-1 shadow-[0_8px_30px_-12px_oklch(0_0_0/60%)] backdrop-blur-md"
        >
          <ProfileTabs compact className="border-0 bg-transparent p-0" />
        </motion.div>
      ) : null}
    </AnimatePresence>
  )
}
