'use client'

import { Tabs } from '@base-ui/react/tabs'
import { useReducedMotion } from 'motion/react'
import { type ComponentType, useEffect, useRef, useState } from 'react'

import { cn } from '@/lib/utils'

import { IDEAS as SITE_IDEAS, type Idea as SiteIdea } from '../thanksgiving/ideas'
import type { MarkProps } from '../thanksgiving/marks'

/*
 * A workbench for the Thanksgiving mark: one tab per idea the site rotates through, each shown large and at the real
 * header and footer sizes, plus a side-by-side tab. Not linked from anywhere and kept out of search.
 */

type Idea = SiteIdea & {
  level: 'Subtle' | 'Medium' | 'Playful'
  summary: string
  pros: string[]
  cons: string[]
}

const NOTES: Record<string, Omit<Idea, keyof SiteIdea>> = {
  strut: {
    level: 'Playful',
    summary:
      'The mark becomes the turkey. The three stems fan out into tail feathers, four more grow in behind them, and the dots fold into the body, head and wattle. A beak, feet and blinking eyes pop in last.',
    pros: [
      'Built from the mark itself, so it reads as iii first and turkey second',
      'Feels alive while hovered: the tail breathes, the head gobbles, the eyes blink',
    ],
    cons: ['The busiest of the set at 24px'],
  },
  gobble: {
    level: 'Subtle',
    summary:
      'The mark stays the mark. The middle dot becomes a small turkey head (comb, beak, wattle, eye), the outer dots warm to orange and amber, and while held the head pecks forward every couple of seconds.',
    pros: ['Closest to the brand; the stems never move'],
    cons: ['Details are small at header size'],
  },
  'turkey-cube': {
    level: 'Playful',
    summary:
      'The Halloween engine: the bars fold into a square that turns like a cube to show a pixel turkey, its tail fanned out behind in red, orange and amber rings.',
    pros: ['Clear continuity with October'],
    cons: ['The mark disappears behind a face while it plays'],
  },
  lineup: {
    level: 'Playful',
    summary:
      'The cube with a rotating cast: turkey, pumpkin pie, maple leaf and corn. Each play shows the next face, like the Halloween monsters did.',
    pros: ['Rewards repeat visits'],
    cons: ['Busiest overall'],
  },
  trot: {
    level: 'Medium',
    summary:
      'The mark stays untouched. A tiny turkey walks in from the left along the tops of the dots, hopping the gaps, and each dot dips under its feet as it passes. Plays once.',
    pros: ['Charming, and the mark is never altered'],
    cons: ['The turkey is about 8px tall in the header'],
  },
  chorus: {
    level: 'Medium',
    summary:
      'All three dots become turkey heads in slightly different browns. They pop in left to right and, while held, peck one after another like a round.',
    pros: ['The canon reads even at small sizes; stems stay untouched'],
    cons: ['The right beak pokes past the mark edge'],
  },
}

const IDEAS: Idea[] = SITE_IDEAS.map((idea) => ({ ...idea, ...NOTES[idea.id] }))

const LEVEL_STYLE: Record<Idea['level'], string> = {
  Subtle: 'text-foreground/70',
  Medium: 'text-[#ebb24b]',
  Playful: 'text-[#e5852e]',
}

/** Hover or focus plays the mark; `forced` holds it on from the panel controls. */
function Hoverable({
  Mark,
  size,
  forced,
  still,
  t,
  speed,
  label,
  className,
}: {
  Mark: ComponentType<MarkProps>
  size: number
  forced: boolean
  still: boolean
  t?: number
  speed?: number
  label: string
  className?: string
}) {
  const [hovered, setHovered] = useState(false)
  // Readers who asked for less motion get the still frame, as they would on the site.
  const reduceMotion = useReducedMotion()
  return (
    <button
      type="button"
      aria-label={label}
      className={cn(
        'inline-flex cursor-default items-center rounded-md outline-none focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-4',
        className,
      )}
      // Touch fires a hover on tap; only a real pointer hovering should play the mark.
      onPointerEnter={(e) => e.pointerType === 'mouse' && setHovered(true)}
      onPointerLeave={(e) => e.pointerType === 'mouse' && setHovered(false)}
      onFocus={() => setHovered(true)}
      onBlur={() => setHovered(false)}
    >
      <Mark on={hovered || forced} size={size} still={still || !!reduceMotion} t={t} speed={speed} />
    </button>
  )
}

const control =
  'h-8 rounded-md border border-border px-3 text-[13px] text-foreground/80 transition-colors hover:bg-foreground/[0.06] hover:text-foreground focus-visible:outline-2 focus-visible:outline-foreground'

/** The same idea at three sizes, sharing one set of controls. */
function IdeaPanel({ idea }: { idea: Idea }) {
  const [held, setHeld] = useState(false)
  const [visit, setVisit] = useState(false)
  const [still, setStill] = useState(false)
  const [speed, setSpeed] = useState(1)
  const [scrub, setScrub] = useState<number | null>(null)
  const timer = useRef<number>(undefined)
  useEffect(() => () => window.clearTimeout(timer.current), [])

  const playOnce = () => {
    setScrub(null)
    window.clearTimeout(timer.current)
    setVisit(false)
    requestAnimationFrame(() => {
      setVisit(true)
      timer.current = window.setTimeout(() => setVisit(false), idea.oneshot ? 400 : 2600 / speed)
    })
  }
  const forced = held || visit
  const shared = { forced, still, t: scrub ?? undefined, speed }

  return (
    <div className="mt-8 grid gap-6 lg:grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)]">
      <div className="flex flex-col gap-4">
        <div className="relative flex h-[340px] items-center justify-center overflow-hidden rounded-xl border border-border bg-card">
          <Hoverable Mark={idea.Mark} size={112} {...shared} label={`${idea.name}, large preview`} />
          <p className="absolute bottom-3 left-4 text-[12px] text-muted-foreground">Hover or focus the mark · 112px</p>
        </div>
        <div className="flex flex-wrap gap-2">
          <button type="button" className={control} onClick={playOnce}>
            Play once
          </button>
          {!idea.oneshot && (
            <button
              type="button"
              className={cn(control, held && 'bg-foreground/10 text-foreground')}
              onClick={() => setHeld((h) => !h)}
              aria-pressed={held}
            >
              {held ? 'Release' : 'Hold on'}
            </button>
          )}
          <button
            type="button"
            className={cn(control, still && 'bg-foreground/10 text-foreground')}
            onClick={() => setStill((s) => !s)}
            aria-pressed={still}
          >
            Reduced-motion still
          </button>
        </div>
        {!idea.springs && (
          <div className="flex flex-wrap items-center gap-x-5 gap-y-3">
            <fieldset className="flex items-center gap-1">
              <legend className="sr-only">Playback speed</legend>
              {[1, 0.5, 0.25].map((rate) => (
                <button
                  key={rate}
                  type="button"
                  aria-pressed={speed === rate}
                  className={cn(control, 'px-2.5 font-mono', speed === rate && 'bg-foreground/10 text-foreground')}
                  onClick={() => setSpeed(rate)}
                >
                  {rate === 1 ? '1×' : rate === 0.5 ? '½×' : '¼×'}
                </button>
              ))}
            </fieldset>
            <label className="flex min-w-[220px] flex-1 items-center gap-3 text-[12px] text-muted-foreground">
              Scrub
              <input
                type="range"
                min={0}
                max={1}
                step={0.005}
                value={scrub ?? 0}
                onChange={(e) => setScrub(Number(e.target.value))}
                className="h-1 flex-1 cursor-pointer accent-[#e5852e]"
              />
              <span className="w-10 text-right font-mono text-foreground/70">
                {scrub === null ? 'live' : `${Math.round(scrub * 100)}%`}
              </span>
              {scrub !== null && (
                <button type="button" className={cn(control, 'h-7 px-2')} onClick={() => setScrub(null)}>
                  Live
                </button>
              )}
            </label>
          </div>
        )}
      </div>

      <div className="flex flex-col gap-4">
        <div>
          <div className="flex items-center gap-2 text-[12px]">
            <span className={cn('font-medium', LEVEL_STYLE[idea.level])}>{idea.level}</span>
            {idea.oneshot && <span className="text-muted-foreground">· plays once</span>}
          </div>
          <p className="mt-2 text-pretty text-[15px] text-foreground/85 leading-relaxed">{idea.summary}</p>
        </div>

        <p className="text-[12px] text-muted-foreground">In the header · 24px</p>
        <div className="flex h-14 items-center gap-6 rounded-xl border border-border bg-background/80 pr-2 pl-5">
          <Hoverable Mark={idea.Mark} size={24} {...shared} label={`${idea.name}, header size`} />
          <nav className="hidden gap-5 text-[14px] text-foreground/70 sm:flex" aria-hidden>
            <span>Docs</span>
            <span>Blog</span>
            <span>Roadmap</span>
            <span>Community</span>
          </nav>
          <span className="ml-auto rounded-lg bg-foreground px-3 py-1.5 text-[13px] text-background">Get started</span>
        </div>

        <p className="text-[12px] text-muted-foreground">In the footer · 32px</p>
        <div className="rounded-xl border border-border bg-background px-5 pt-6 pb-5">
          <Hoverable Mark={idea.Mark} size={32} {...shared} label={`${idea.name}, footer size`} />
          <p className="mt-4 max-w-[320px] text-[14px] text-muted-foreground leading-relaxed">
            A next-generation software system. Workers. Triggers. Functions.
          </p>
        </div>

        <div className="grid gap-4 sm:grid-cols-2">
          <List title="Good" items={idea.pros} />
          <List title="Watch out" items={idea.cons} />
        </div>
      </div>
    </div>
  )
}

function List({ title, items }: { title: string; items: string[] }) {
  return (
    <div>
      <p className="text-[12px] text-muted-foreground">{title}</p>
      <ul className="mt-1.5 space-y-1 text-[13px] text-foreground/80 leading-snug">
        {items.map((item) => (
          <li key={item} className="flex gap-2">
            <span aria-hidden className="text-muted-foreground">
              ·
            </span>
            {item}
          </li>
        ))}
      </ul>
    </div>
  )
}

/** Every idea at once, so they can be compared on one screen; "Play all" triggers them together. */
function Compare() {
  const [all, setAll] = useState(false)
  const [still, setStill] = useState(false)
  const timer = useRef<number>(undefined)
  useEffect(() => () => window.clearTimeout(timer.current), [])
  const playAll = () => {
    window.clearTimeout(timer.current)
    setAll(true)
    timer.current = window.setTimeout(() => setAll(false), 2600)
  }

  return (
    <div className="mt-8">
      <div className="flex flex-wrap gap-2">
        <button type="button" className={control} onClick={playAll}>
          Play all
        </button>
        <button
          type="button"
          className={cn(control, still && 'bg-foreground/10 text-foreground')}
          onClick={() => setStill((s) => !s)}
          aria-pressed={still}
        >
          Reduced-motion stills
        </button>
      </div>
      <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {IDEAS.map((idea) => (
          <div key={idea.id} className="flex flex-col rounded-xl border border-border bg-card">
            <div className="flex h-[200px] items-center justify-center">
              <Hoverable Mark={idea.Mark} size={64} forced={all} still={still} label={idea.name} />
            </div>
            <div className="flex items-center justify-between border-border border-t px-4 py-3 text-[13px]">
              <span className="text-foreground">{idea.name}</span>
              <span className={LEVEL_STYLE[idea.level]}>{idea.level}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

const TABS = [...IDEAS.map((i) => i.id), 'all']

export function SeasonalLab() {
  const [tab, setTab] = useState(IDEAS[0].id)

  // Deep links: /lab/seasonal-logo#gobble opens that tab. Read after mount so the static HTML stays the same.
  useEffect(() => {
    const hash = window.location.hash.slice(1)
    if (TABS.includes(hash)) setTab(hash)
  }, [])

  const select = (value: string) => {
    setTab(value)
    window.history.replaceState(null, '', `#${value}`)
  }

  return (
    <Tabs.Root value={tab} onValueChange={(value) => select(String(value))}>
      <Tabs.List className="relative flex w-full flex-wrap gap-1 rounded-xl border border-border bg-card p-1">
        {IDEAS.map((idea, i) => (
          <Tabs.Tab
            key={idea.id}
            value={idea.id}
            className="relative z-10 flex h-9 shrink-0 items-center gap-2 rounded-lg px-3.5 text-[14px] text-foreground/60 outline-none transition-colors hover:text-foreground focus-visible:outline-2 focus-visible:outline-foreground data-[active]:text-foreground"
          >
            <span className="font-mono text-[11px] text-muted-foreground">{i + 1}</span>
            {idea.name}
          </Tabs.Tab>
        ))}
        <Tabs.Tab
          value="all"
          className="relative z-10 ml-auto flex h-9 shrink-0 items-center rounded-lg px-3.5 text-[14px] text-foreground/60 outline-none transition-colors hover:text-foreground focus-visible:outline-2 focus-visible:outline-foreground data-[active]:text-foreground"
        >
          Side by side
        </Tabs.Tab>
        <Tabs.Indicator className="absolute top-[var(--active-tab-top)] left-0 h-[var(--active-tab-height)] w-[var(--active-tab-width)] translate-x-[var(--active-tab-left)] rounded-lg bg-foreground/[0.08] transition-[translate,width] duration-200 ease-[cubic-bezier(0.23,1,0.32,1)]" />
      </Tabs.List>

      {IDEAS.map((idea) => (
        <Tabs.Panel key={idea.id} value={idea.id} className="outline-none">
          <IdeaPanel idea={idea} />
        </Tabs.Panel>
      ))}
      <Tabs.Panel value="all" className="outline-none">
        <Compare />
      </Tabs.Panel>
    </Tabs.Root>
  )
}
