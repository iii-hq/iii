# component registry

every shared file under `src/` has exactly one entry here (heading = file
basename). before building any visual for a deck, read this file — **reuse
first**. to add a component, meet the checklist in the presentation skill's
`reference/component-standards.md`, then append an entry in alphabetical order
within its kind. `node build.mjs` warns when a src file has no entry (or an
entry has no file); `--strict-registry` turns the warning into a failure.

kinds, in section order: **layout · primitive · archetype · hook · util · gallery**

## layout

### Footer
- kind: layout
- import: `import { Footer } from '@lib/components/Footer'`
- purpose: deck footer — eyebrow, big closing line, command chip, attribution bar
- props: `{ footer: FooterSpec }` (from the deck's `content/deck.ts`)
- use when: every deck; wired once in App.tsx
- used by: all decks

### PageShell
- kind: layout
- import: `import { PageShell } from '@lib/components/PageShell'`
- purpose: deep-dive page wrapper — eyebrow, title, description, prose column
- props: `{ eyebrow: string; title: string; description: ReactNode; children }`
- use when: any `#/<slug>` deep-dive page
- used by: 2026-06-codegen, 2026-06-rbac-proxy-worker

### PlayerControls
- kind: layout
- import: `import { PlayerControls } from '@lib/components/PlayerControls'`
- purpose: stepper transport bar (prev/next/play) under steppable diagrams
- props: `{ stepper: Stepper; total: number; label?: string; className?: string }`
- use when: any diagram driven by `useStepper`
- used by: all decks (via archetypes)

### Section
- kind: layout
- import: `import { Section } from '@lib/components/Section'`
- purpose: numbered home-page section shell with reveal-on-scroll
- props: `{ id: string; index: string; eyebrow: string; title: ReactNode; lede?: ReactNode; children }`
- use when: every home-page slide; `id` must match a NAV entry
- used by: all decks

### SpecSheet
- kind: layout
- import: `import { SpecSheet, SpecRow } from '@lib/components/SpecSheet'`
- purpose: closed-by-default `<details>` depth layer — execs skim, engineers drill
- props: `{ title: ReactNode; meta?: ReactNode; defaultOpen?: boolean; children }`; `SpecRow { name: string; children }`
- use when: schema/field/config detail behind a slide's single claim
- used by: all decks

### TopNav
- kind: layout
- import: `import { TopNav } from '@lib/components/TopNav'`
- purpose: sticky top nav — wordmark, scroll-spy section links, spec link, theme toggle
- props: `{ route: Route; meta: DeckMeta; nav: NavItem[]; specHref?: string | null }`
- use when: every deck; wired once in App.tsx (viewer passes `specHref={null}`)
- used by: all decks, _viewer

## primitive

### Button
- kind: primitive
- import: `import { Button } from '@lib/components/schematic/Button'`
- purpose: border-driven button, no rounded corners; primary = the one filled accent
- props: `{ variant?: ButtonVariant; size?: ButtonSize }` + anchor/button attrs
- use when: CTAs; ration primary to one per screen
- used by: all decks

### Caret
- kind: primitive
- import: `import { Caret } from '@lib/components/schematic/Caret'`
- purpose: blinking terminal cursor
- props: `{ className?: string }`
- use when: implying a live prompt in terminal chrome
- used by: all decks

### Cell
- kind: primitive
- import: `import { Cell } from '@lib/components/schematic/Cell'`
- purpose: bordered grid cell for `gap-px bg-rule` mosaics
- props: `{ title?: ReactNode; children; className?; bodyClassName? }`
- use when: problem grids, capability mosaics, failure cards
- used by: all decks

### CodeBlock
- kind: primitive
- import: `import { CodeBlock, K, S, C, M } from '@lib/components/schematic/CodeBlock'`
- purpose: titled code block; inline `<K>/<S>/<C>/<M>` tags color keywords/strings/comments/muted
- props: `{ title?: ReactNode; children }`
- use when: config/pseudocode walkthroughs authored as JSX
- used by: all decks

### FnChip
- kind: primitive
- import: `import { FnChip } from '@lib/components/schematic/FnChip'`
- purpose: monospace identifier chip (function ids keep their casing)
- props: `{ tone?: ChipTone; children }`
- use when: naming `worker::function` ids inline or in grids
- used by: all decks

### ModeToggle
- kind: primitive
- import: `import { ModeToggle } from '@lib/components/schematic/ModeToggle'`
- purpose: bordered segmented toggle (theme, policy modes, language tracks)
- props: `{ value: T; onChange: (next: T) => void; options: ModeToggleOption<T>[]; className? }`
- use when: switching between 2–4 named modes; active = accent border + text
- used by: all decks, gallery

### Prompt
- kind: primitive
- import: `import { Prompt } from '@lib/components/schematic/Prompt'`
- purpose: terminal prompt symbol prefix (`$`, `//`)
- props: `{ symbol?: string; className?: string; children? }`
- use when: eyebrows and command lines
- used by: all decks, gallery

### Sheet
- kind: primitive
- import: `import { Sheet } from '@lib/components/schematic/Sheet'`
- purpose: the centered max-w-[1200px] drafting sheet with left/right rules
- props: `{ children; className? }`
- use when: the root shell of every page; use container queries inside, not viewport
- used by: all decks, gallery, _viewer

### StatusDot
- kind: primitive
- import: `import { StatusDot } from '@lib/components/schematic/StatusDot'`
- purpose: status dot, optional pulse animation
- props: `{ tone?: DotTone; pulse?: boolean }`
- use when: live/running/draft indicators
- used by: all decks, gallery

### StatusPanel
- kind: primitive
- import: `import { StatusPanel } from '@lib/components/schematic/StatusPanel'`
- purpose: bordered status callout (alert/accent tones) with headline + detail
- props: `{ variant?: StatusVariant; icon?: ReactNode; headline: ReactNode; detail?: ReactNode; className? }`
- use when: failure modes, guarantees, fail-closed statements
- used by: all decks

### Terminal
- kind: primitive
- import: `import { Terminal, TerminalRow } from '@lib/components/schematic/Terminal'`
- purpose: terminal window with staggered `fade-rise` output lines
- props: `{ title?: ReactNode; children; className? }`; `TerminalRow { command: ReactNode; output?: ReactNode; showCaret?: boolean }`
- use when: install/run sequences; prefer CliPlayground for multi-track playback
- used by: all decks

### Wordmark
- kind: primitive
- import: `import { Wordmark } from '@lib/components/schematic/Wordmark'`
- purpose: the iii brand mark
- props: `{ className?: string }`
- use when: nav + footer chrome only
- used by: all decks, gallery, _viewer

### WorkerCard
- kind: primitive
- import: `import { WorkerCard } from '@lib/components/schematic/WorkerCard'`
- purpose: one worker as a product card — name, version, description, install command, kind
- props: `{ name: string; version?: string; description: ReactNode; command: ReactNode; kind: string; focused?: boolean; className? }`
- use when: cataloguing installable workers/components
- used by: (available — promoted from the agentic deck's system)

## archetype

### CliPlayground
- kind: archetype
- import: `import { CliPlayground } from '@lib/components/diagrams/CliPlayground'`
- purpose: A3 — terminal playback with switchable tracks, staggered fade-rise lines
- props: `{ tracks: CliTrack[]; title?: string; intervalMs?: number; className? }`
- use when: the spec's win is a command-line flow with variants (langs, modes)
- used by: 2026-06-codegen, 2026-06-rbac-proxy-worker

### DurabilityTimeline
- kind: archetype
- import: `import { DurabilityTimeline } from '@lib/components/diagrams/DurabilityTimeline'`
- purpose: A16 — steppable lifecycle timeline where each stage narrates an evolving state record
- props: `{ stages: TimelineStage[]; heading: string; headingNote?: string; recordHeading: string; intervalMs?; className? }`
- use when: a durable thing survives crashes/waits and you can show its record at each stage
- not when: state just accumulates with no record panel (use StepReveal)
- used by: 2026-06-agentic

### EventFanOut
- kind: archetype
- import: `import { EventFanOut } from '@lib/components/diagrams/EventFanOut'`
- purpose: A17 — one concrete write → trigger type → N named subscribers, ambient animation
- props: `{ heading; source; sourceSub?; trigger; handlers: FanOutHandler[]; edges: FanOutEdge[]; footnote?; ariaLabel; … }`
- use when: narrating a specific reactive write with named handlers, always-on
- not when: you need an abstract many-to-one/one-to-many with steppable ripples (use FanOut)
- used by: 2026-06-agentic

### FanOut
- kind: archetype
- import: `import { FanOut } from '@lib/components/diagrams/FanOut'`
- purpose: A7 — one write fans to many handlers, ripple + marching edges
- props: `{ source: {label, sub?}; trigger: string; handlers: FanHandler[]; … }`
- use when: the reactive surface is the claim (subscribe once, everything updates)
- used by: 2026-06-codegen, 2026-06-rbac-proxy-worker

### Funnel
- kind: archetype
- import: `import { Funnel } from '@lib/components/diagrams/Funnel'`
- purpose: A8 — many paths converge on one target (optional rejected path)
- props: `{ title?: string; paths: FunnelPath[]; target: {label, sub?} }`
- use when: consolidation claims — N ways in, one enforced way through
- used by: 2026-06-codegen, 2026-06-rbac-proxy-worker

### SequencePlayer
- kind: archetype
- import: `import { SequencePlayer } from '@lib/components/diagrams/SequencePlayer'`
- purpose: A5 — step-through sequence diagram: lifelines per lane, one arrow per step, narration
- props: `{ title: string; lanes: SeqLane[]; steps: SeqStep[]; width?; intervalMs?; className? }`
- use when: a temporal protocol, turn loop, handshake, numbered steps
- used by: 2026-06-agentic, 2026-06-codegen, 2026-06-rbac-proxy-worker

### SpawnTree
- kind: archetype
- import: `import { SpawnTree } from '@lib/components/diagrams/SpawnTree'`
- purpose: A18 — parent spawns N parallel children, joins their results; steppable narrative states
- props: `{ heading; chips?; parentTitle; parentLabels; parentCallLine; nodes: SpawnTreeChild[]; states: SpawnTreeState[]; ariaLabel; … }`
- use when: fan-out/join concurrency with a parked parent is the claim
- used by: 2026-06-agentic

### StepReveal
- kind: archetype
- import: `import { StepReveal } from '@lib/components/diagrams/StepReveal'`
- purpose: A6 — lifecycle stepper with an evolving state panel
- props: `{ title: string; stages: RevealStage[]; intervalMs?; className? }`
- use when: a linear lifecycle where each stage adds/changes state
- not when: you need the record-beside-narration density of DurabilityTimeline
- used by: 2026-06-codegen, 2026-06-rbac-proxy-worker

### SystemMap
- kind: archetype
- import: `import { SystemMap, MapDatasheet } from '@lib/components/diagrams/SystemMap'`
- purpose: A4 — interactive node/edge architecture map; click a node → datasheet
- props: `{ nodes: MapNode[]; edges: MapEdge[]; selected: string; onSelect: (id) => void; width?; }`; `MapDatasheet { info: MapNodeInfo }`
- use when: the architecture overview slide (nearly every deck's opener)
- used by: 2026-06-codegen, 2026-06-rbac-proxy-worker (agentic ships a deck-local fork)

## hook

### useHashRoute
- kind: hook
- import: `import { useHashRoute, type Route } from '@lib/hooks/useHashRoute'`
- purpose: hash routing — `#/` home with scroll anchors, `#/<slug>[/rest]` pages
- props: returns `Route = { kind: 'home' } | { kind: 'page'; slug: string; rest: string[] }`
- use when: every deck App.tsx; SpecPage reads `rest[0]` for `#/spec/<file>`
- used by: all decks, _viewer

### useStepper
- kind: hook
- import: `import { useStepper, type Stepper } from '@lib/hooks/useStepper'`
- purpose: autoplaying step state for diagram archetypes (goTo/next/prev/pause)
- props: `useStepper(total: number, intervalMs: number)`
- use when: any steppable diagram; pair with PlayerControls
- used by: all decks (via archetypes)

### useTheme
- kind: hook
- import: `import { useTheme } from '@lib/hooks/useTheme'`
- purpose: light/dark theme state persisted to localStorage, sets `data-theme`
- props: returns `[theme, setTheme]`
- use when: chrome with a theme toggle (TopNav/SiteHeader already wire it)
- used by: all decks, gallery, _viewer

## util

### deck-types
- kind: util
- import: `import type { DeckMeta, FooterSpec, NavItem } from '@lib/lib/deck-types'`
- purpose: the shapes a deck's `content/deck.ts` feeds the shared chrome
- props: types only
- use when: typing a deck's DECK_META / NAV / FOOTER
- used by: all decks

### highlight
- kind: util
- import: `import { Highlight, HighlightStyles, type HlLang } from '@lib/content/highlight'`
- purpose: syntax highlighter (ts/js/rust/python/yaml; monochrome fallback)
- props: `Highlight { code: string; lang: HlLang }`; mount `HighlightStyles` once per page
- use when: highlighted code from string data (CodeBlock covers JSX-authored code)
- used by: all decks (markdown), 2026-06-codegen

### markdown
- kind: util
- import: `import { Markdown } from '@lib/content/markdown'`
- purpose: trusted spec markdown → React in the drafting-sheet system; strips leading frontmatter; ```mermaid fences render live
- props: `{ source: string }`
- use when: rendering spec md (SpecPage/viewer do this for you)
- used by: SpecPage, _viewer

### mermaid
- kind: util
- import: `import { Mermaid } from '@lib/content/mermaid'`
- purpose: theme-aware live mermaid diagram (lazy-loads mermaid on first render)
- props: `{ chart: string }`
- use when: mermaid fences in spec md (wired via markdown)
- used by: markdown

### SpecPage
- kind: util
- import: `import { SpecPage } from '@lib/pages/SpecPage'`
- purpose: A15 — the `#/spec` page: file sidebar + rendered spec markdown
- props: `{ docs: Record<string, string> }` — pass the deck's `spec-docs.ts` glob
- use when: every deck (`spec` entry in PAGES); the md-only viewer reuses it
- used by: all decks, _viewer

### utils
- kind: util
- import: `import { cn } from '@lib/lib/utils'`
- purpose: `cn()` — clsx + tailwind-merge class combiner
- props: `cn(...inputs)`
- use when: any conditional className
- used by: everything

## gallery

### Gallery
- kind: gallery
- import: `import { Gallery } from '@lib/gallery/Gallery'` (gallery app only)
- purpose: the spec card grid, rendered from `virtual:spec-manifest`
- props: none (reads SPECS)
- use when: gallery app only
- used by: gallery

### PresentationCard
- kind: gallery
- import: `import { PresentationCard } from '@lib/gallery/PresentationCard'`
- purpose: one spec card — number, deck/spec badge, title, tagline, tags, open →
- props: `{ spec: SpecEntry; index: number }`
- use when: gallery app only
- used by: gallery

### site
- kind: gallery
- import: `import { SITE } from '@lib/gallery/site'`
- purpose: the gallery's repo identity (wordmark label, hero copy, attribution) — set once
- props: data only
- use when: gallery chrome; never per-spec data
- used by: gallery

### SiteFooter
- kind: gallery
- import: `import { SiteFooter } from '@lib/gallery/SiteFooter'`
- purpose: gallery footer bar (attribution + source of truth)
- props: none
- use when: gallery app only
- used by: gallery

### SiteHeader
- kind: gallery
- import: `import { SiteHeader } from '@lib/gallery/SiteHeader'`
- purpose: gallery sticky header — wordmark, label, theme toggle
- props: none
- use when: gallery app only
- used by: gallery
