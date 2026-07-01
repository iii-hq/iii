# component standards — deck-local vs promoted, and the registry

The shared library (`<base>/src/`) is the single copy of every reusable
visual. Decks never duplicate it and never reach into each other. This file
defines when a new component may join it, what it must satisfy, and how the
registry stays truthful.

## The decision rule

**Default: deck-local.** A new visual lives at
`<base>/<slug>/src/diagrams/<Name>.tsx`, imported relatively. It can bake in
its spec's copy and topology — it ships with one deck only.

**Promote to `<base>/src/components/{diagrams,schematic}/` only when ALL
three hold:**

1. **Generic over its data** — nothing spec-specific inside; every label,
   node, stage, and copy string arrives via typed props (the deck keeps the
   data in its `content/`).
2. **Recurring shape** — it maps to a spec shape future decks will plausibly
   need (a lifecycle, a tree, a timeline, a fan-out, a funnel…), not this
   spec's particular topology.
3. **Passes the checklist below** without deck-specific hacks.

A second deck needing a deck-local diagram is the natural promotion trigger:
promote it then (props-decouple first), never copy it.

**Never fork.** Don't copy a shared component into a deck to tweak it —
extend it with additive, non-breaking props, or build a genuinely different
deck-local one. **Changing an existing shared component's behavior or API
requires explicit user approval and a full `node build.mjs`** — it re-renders
every deck on the site.

## The promotion checklist

1. Props-driven; zero content/data inside the component.
2. Design tokens only (`bg-bg`, `text-ink-faint`, `stroke-rule`, …) — never
   hardcoded hex; light/dark comes free.
3. Any auto/infinite animation gated behind a runtime
   `prefers-reduced-motion` check; the static fallback reads complete.
4. Keyboard operable; controls disable at bounds; visible active state.
5. `aria-label` on the interactive figure; sensible roles.
6. Container queries (`@3xl:` …), never viewport breakpoints.
7. Wide content wrapped in `overflow-x-auto` with an inner `min-w-[…]`; the
   page body never scrolls horizontally.
8. No shadows, no gradients (`deal-shadow` is the sole exception); radii 0.
9. Exported prop types + a one-line JSDoc header stating purpose (the source
   of the registry entry's `purpose` line).
10. Its `COMPONENTS.md` entry lands **in the same change**.

## The registry (`<base>/COMPONENTS.md`)

One entry per file; the `### <heading>` equals the file's basename — that 1:1
rule is what makes the parity check trivial (`node build.mjs` warns on files
without entries and entries without files; `--strict-registry` fails).

Six sections in fixed order — **layout · primitive · archetype · hook · util ·
gallery** — alphabetical within each. Append at the right position; never
reword other entries in a deck PR; delete an entry only when deleting its file.

Entry template:

```markdown
### SequencePlayer
- kind: archetype
- import: `import { SequencePlayer } from '@lib/components/diagrams/SequencePlayer'`
- purpose: step-through sequence diagram — lifelines per lane, one arrow per step, narrated
- props: `{ title: string; lanes: SeqLane[]; steps: SeqStep[] }`
- use when: the spec has a temporal protocol, a turn loop, a handshake, numbered steps
- not when: state evolves in place with no message passing (use StepReveal)
- used by: 2026-06-agentic, 2026-06-codegen
```

`props` is a compact sketch (the file is the type truth); `use when` speaks
the archetype-selection voice; `not when` disambiguates near-siblings
(DurabilityTimeline vs StepReveal, EventFanOut vs FanOut); `used by` is
informative, maintained by the skill when it wires a deck.

## Worked precedents

- **WorkerCard** — already props-driven when promoted from the agentic deck:
  the model promotion (zero refactor).
- **DurabilityTimeline / EventFanOut / SpawnTree** — promoted from the agentic
  deck by moving their baked STAGES/HANDLERS/STATES arrays into the deck's
  `content/{durability,fanout,spawn}.ts` and threading them as props.
- **agentic's SystemMap fork** — NOT promoted: 483 lines diverged from the
  shared SystemMap with deck content baked in. It stays at
  `2026-06-agentic/src/diagrams/SystemMap.tsx`, the canonical props-driven
  `@lib` SystemMap serves everyone else.
