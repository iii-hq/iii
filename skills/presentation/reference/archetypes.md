# Archetype library

The deck is assembled from a fixed catalog of proven slide archetypes. For
every slide: **match the spec content to an archetype, then supply data.** Do
not invent a new diagram when one fits. Hand-build a bespoke SVG only when no
archetype matches and the concept is load-bearing — and then follow
`SequencePlayer.tsx`'s conventions.

Two kinds:
- **Diagram archetypes** — shared, props-driven components in
  `@lib/components/diagrams/` (the base project's library). You import them
  and pass data from the deck's `content/`.
- **Compositional archetypes** — patterns you build inline in a section from
  primitives (`Cell`, `SpecSheet`, `CodeBlock`, `ModeToggle`, `Button`, …).
  The starter `Hero.tsx` and `PayoffSection.tsx` are worked examples.

**The repo registry wins.** This catalog documents the bundled baseline; the
live inventory is `<base>/COMPONENTS.md`, which grows as decks promote new
archetypes (per `reference/component-standards.md`). Read it in Phase 1b —
when it and this file disagree, the registry is the truth.

## Selection heuristics

| The spec has… | Use |
|---|---|
| a thesis / one-line promise | A1 Hero |
| a "today's pain" enumeration | A2 Problem + A11 |
| a command / install / golden path | A3 CLI Playground |
| a component/responsibility map, planes, "owns / never does" | A4 System Map |
| a temporal protocol, a "turn", a handshake, numbered steps | A5 Sequence |
| a lifecycle whose state evolves (boot, durability, readiness) | A6 Step Reveal |
| one event → many handlers; reactive/triggers | A7 Fan-Out |
| many cases collapse to one mechanism (or one forks) | A8 Funnel |
| a mode/policy that changes what's allowed | A9 Toggle Explorer |
| a human-in-the-loop / approval / branching outcome | A10 Decision Flow |
| before/after, removed, renamed, metrics | A11 Scorecard + Solves |
| any field-level schema (the depth layer of any slide) | A12 Spec Datasheet |
| a config file, pseudocode, an API skeleton | A13 Code Walkthrough |
| a distinct consumer/scenario worth a full page | A14 Deep-dive Page |

Every interactive archetype is keyboard-operable, disables controls at bounds,
respects `prefers-reduced-motion`, and degrades to a readable static state.

---

## Diagram archetypes (bundled)

### A4 — System Map  ·  `diagrams/SystemMap.tsx`

The signature visual: the whole architecture in one interactive diagram. Click
a node → its edges highlight and its datasheet updates. Author node `x/y/w/h`
and edge `d` paths in the same coordinate space (default `1030×600`).

```ts
type MapNodeKind = 'primary' | 'secondary' | 'optional' | 'external'
interface MapNode { id; x; y; w; h; title; sub?; kind; tag? }
interface MapEdge { id; from; to; d; label?; lx?; ly?; anchor?; dashed?; dur? }
interface MapNodeInfo { id; kindLabel; role; sections?; note?; bullets?; install? }
interface InfoSection { heading; items: { name; desc? }[]; dotted? }
```

Use in a section with the paired-layout pattern from
`sections/SystemMapSection.tsx` (ResizeObserver sticks the datasheet beside the
map ≥1024px, stacks below it narrower). `MapDatasheet` takes a resolved
`info` object — look it up from a `Record<string, MapNodeInfo>` by `selected`.
Maps from: the spec's architecture/component-responsibility section. One node
per component; `sections` from each component's surface/emits; `dotted: true`
for event lists.

### A5 — Sequence  ·  `diagrams/SequencePlayer.tsx`

A step-through sequence diagram: lifelines per lane, one arrow per step, the
active step in accent with a travelling pulse and a narration line.

```ts
interface SeqLane { id; label; x }
interface SeqStep { from; to; label; title; desc; event? }   // from===to => self-loop
<SequencePlayer title lanes={SEQ_LANES} steps={SEQ_STEPS} />
```

Maps from: any protocol / turn loop / handshake. One lane per participant; one
step per message. Set `event` when a step emits a trigger.

### A6 — Step Reveal  ·  `diagrams/StepReveal.tsx`

A lifecycle/timeline walker: a strip of ordered stages; the stepper walks them;
a panel shows the active stage's evolving record.

```ts
interface RevealStage { label; tone?: 'ink'|'accent'|'alert'|'warn'; caption?; rows?: {k;v}[]; note? }
<StepReveal title stages={STAGES} />
```

Maps from: boot sequences, durability/crash-resume timelines, readiness
bring-up. Use `tone: 'alert'` for a crash/failure stage, `'accent'` for the
resolved one. `rows` is the state object that changes per stage.

### A3 — CLI Playground  ·  `diagrams/CliPlayground.tsx`

An interactive terminal: lines reveal one at a time (staggered `fade-rise`);
switch tracks (golden path vs day-2) with a toggle.

```ts
interface CliLine { cmd?; out?: string[]; fn?; exit? }   // out tinted by ✓/✗/! glyph
interface CliTrack { id; label; lines: CliLine[] }
<CliPlayground tracks={TRACKS} title="run it" />
```

Maps from: install/quickstart, the golden command path, day-2 ops. Put verbatim
commands + output in. `fn` shows the backing function id; prefix output lines
with `✓`/`✗`/`!` to tint them.

### A7 — Fan-Out  ·  `diagrams/FanOut.tsx`

One write/emit fans out to many bound handlers — ambient ripple + marching
edges. No stepper; it just breathes.

```ts
interface FanHandler { id; label; desc? }
<FanOut source={{ label, sub? }} trigger="store::written" handlers={HANDLERS} />
```

Maps from: a reactive/event model. The `trigger` keeps its casing; handlers are
the functions bound to it.

### A8 — Funnel  ·  `diagrams/Funnel.tsx`

Many paths converge into one guaranteed mechanism, with an optional dashed
rejected path.

```ts
interface FunnelPath { id; label; desc? }
<Funnel paths={PATHS} target={{ label, sub? }} reject={{ label, desc? }} />
```

Maps from: "many spawn paths → one owned spawn", "many inputs → one reaper".
Use `reject` to show the eliminated/forbidden path (rendered alert + dashed).

---

## Compositional archetypes (build inline from primitives)

### A1 — Hero  (`sections/Hero.tsx` is the template)

States the **win** in one line + a three-value subhead — never the mechanism.
A stat strip quantifies it; a small `<Terminal>` makes it concrete; two CTAs
jump to the map and the payoff.

### A2 — Problem / Tangle

A grid of `<Cell>` cards naming today's failures, optionally `alert`-tinted, or
a `<StatusPanel variant="alert">` per failure. Make the reader feel the tangle
before the solution. Pull from the spec's pain points / motivation.

### A9 — Toggle Explorer

`useState` for the mode + `<ModeToggle>` + a grid of `<FnChip>`. On change,
re-filter the chips (opacity/strikethrough for blocked) and update a counter.
For policy/allow-list/capability slides.

### A10 — Decision Flow

`useState<'pending'|'approved'|'denied'>` + `<Button>` approve/deny. Borders,
`<StatusDot>`, and the output block change per state; a replay resets. For
human-in-the-loop / governance.

### A11 — Before/After Scorecard + Solves table  (`sections/PayoffSection.tsx`)

A `gap-px bg-rule` metric grid (`before → after`, accent on the after) plus a
two-column problem (alert) / answer (accent) table. The closing argument.

### A12 — Spec Datasheet  (`components/SpecSheet.tsx`)

`<SpecSheet>` + `<SpecRow>` — a `<details>` closed by default. This is the
**depth layer of every slide**: keep the narrative skimmable, put field-level
detail one click away.

### A13 — Code Walkthrough  (`components/schematic/CodeBlock.tsx`)

`<CodeBlock>` with `<K>` keyword / `<S>` string / `<C>` comment / `<M>` muted.
The accent stays rationed to string literals. For config files, pseudocode,
API skeletons.

### A14 — Deep-dive Page  (`components/PageShell.tsx`)

A full `#/<slug>` page via `<PageShell>` (cross-links siblings with `related`).
Register it in `App.tsx`'s `PAGES` map. Give one to each distinct
consumer/scenario worth the full walkthrough.

### A15 — Spec Viewer  (`@lib/pages/SpecPage.tsx`, built-in)

The source-of-truth reading mode: a separate `#/spec/<file>` page that renders
every markdown file of the paired `tech-specs/<slug>/` directory with a file
**sidebar**. The shared `SpecPage` takes `{ docs }`; the deck's tiny
`src/spec-docs.ts` supplies them (an `import.meta.glob` whose literal the
skill substitutes at scaffold — see `reference/hosting.md`). The template
wires it (`spec: Spec` in `PAGES`; the shared `TopNav` renders the `spec`
link). You do not generate or edit it beyond that wiring.

- Markdown → React via `@lib/content/markdown.tsx`, styled in the
  drafting-sheet system; the leading frontmatter block is stripped; fenced
  code reuses `Highlight` where the language is supported
  (`ts/js/rust/python/yaml`), monochrome otherwise.
- ` ```mermaid ` fences render as **live, theme-aware diagrams** via
  `@lib/content/mermaid.tsx` (lazy-loaded; mono font, ink borders, light/dark).
- Sidebar labels come from each file's first H1; README sorts first. Sibling
  `*.md` links rewrite into the viewer; in-page `#anchors` scroll.

It complements the narrative deck: the deck argues the *why* and makes the
architecture interactive; the spec viewer is the *complete text* one click
away. (Specs with no deck get the same reading mode from the base's
`_viewer/` app — every spec URL works either way.)

### A16 — Durability Timeline  (`diagrams/DurabilityTimeline.tsx`)

A lifecycle told as a steppable timeline of stages, each pairing a narration
with an **evolving state record** beside it (status / step / calls / note).
Built for "a durable thing survives crashes and waits" stories.
`{ stages, heading, headingNote?, recordHeading }` — a stage can mark
`gapAfter` to render a dashed pause connector.
**vs A6 Step Reveal:** reach for A16 when the record-beside-narration density
is the point; A6 when state simply accumulates.

### A17 — Event Fan-Out  (`diagrams/EventFanOut.tsx`)

One **concrete, named write** → trigger type → N named subscribers, with
ambient always-on motion (ripples + traveling dots). Denser and more literal
than A7. `{ heading, source, sourceSub?, trigger, handlers, edges, footnote?,
ariaLabel }`.
**vs A7 Fan-Out:** A17 narrates one specific write with real handler names;
A7 is the abstract reactive-surface shape.

### A18 — Spawn Tree  (`diagrams/SpawnTree.tsx`)

A parent spawns N parallel children and joins their results: fan out, run
concurrently, join back, one answer — with steppable narrative states driving
which children exist/run/deliver, and a parked-parent visual state.
`{ heading, chips?, parentTitle, parentLabels, parentCallLine, nodes, states,
ariaLabel }`. Use for sub-agent / fan-out-join concurrency claims.

### Primitive: WorkerCard  (`schematic/WorkerCard.tsx`)

One worker as a product card — name, version, description, install command,
kind badge. Use when cataloguing installable workers/components in a grid.
